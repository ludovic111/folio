//! A small XML tree for the word-processing formats (DOCX, ODT), and helpers to write XML.
//!
//! Elements are named with a fixed prefix per namespace (`w:p`, `r:id`, `text:span`), whatever
//! prefix the file used, so Strict OOXML and odd generators read like everyone else. Names in
//! namespaces this table doesn't know keep the file's own prefix.

use std::collections::HashMap;
use std::io::{Read, Write};

use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::NsReader;

/// Namespace URIs and the prefix the readers use for them.
const NAMESPACES: &[(&str, &str)] = &[
    ("http://schemas.openxmlformats.org/wordprocessingml/2006/main", "w"),
    ("http://purl.oclc.org/ooxml/wordprocessingml/main", "w"),
    ("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "r"),
    ("http://purl.oclc.org/ooxml/officeDocument/relationships", "r"),
    ("http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing", "wp"),
    ("http://purl.oclc.org/ooxml/drawingml/wordprocessingDrawing", "wp"),
    ("http://schemas.openxmlformats.org/drawingml/2006/main", "a"),
    ("http://purl.oclc.org/ooxml/drawingml/main", "a"),
    ("http://schemas.openxmlformats.org/drawingml/2006/picture", "pic"),
    ("http://purl.oclc.org/ooxml/drawingml/picture", "pic"),
    ("http://schemas.openxmlformats.org/drawingml/2006/chart", "c"),
    ("http://schemas.openxmlformats.org/drawingml/2006/diagram", "dgm"),
    ("http://schemas.openxmlformats.org/officeDocument/2006/math", "m"),
    ("http://purl.oclc.org/ooxml/officeDocument/math", "m"),
    ("http://schemas.openxmlformats.org/markup-compatibility/2006", "mc"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingShape", "wps"),
    ("http://schemas.microsoft.com/office/word/2010/wordprocessingGroup", "wpg"),
    ("http://schemas.microsoft.com/office/word/2010/wordml", "w14"),
    ("http://schemas.microsoft.com/office/word/2012/wordml", "w15"),
    ("urn:schemas-microsoft-com:vml", "v"),
    ("urn:schemas-microsoft-com:office:office", "o"),
    ("http://schemas.openxmlformats.org/package/2006/relationships", "rel"),
    ("http://schemas.openxmlformats.org/package/2006/content-types", "ct"),
    ("http://schemas.openxmlformats.org/package/2006/metadata/core-properties", "cp"),
    ("http://purl.org/dc/elements/1.1/", "dc"),
    ("http://purl.org/dc/terms/", "dcterms"),
    ("http://www.w3.org/XML/1998/namespace", "xml"),
    // OpenDocument.
    ("urn:oasis:names:tc:opendocument:xmlns:office:1.0", "office"),
    ("urn:oasis:names:tc:opendocument:xmlns:style:1.0", "style"),
    ("urn:oasis:names:tc:opendocument:xmlns:text:1.0", "text"),
    ("urn:oasis:names:tc:opendocument:xmlns:table:1.0", "table"),
    ("urn:oasis:names:tc:opendocument:xmlns:drawing:1.0", "draw"),
    ("urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0", "fo"),
    ("urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0", "svg"),
    ("urn:oasis:names:tc:opendocument:xmlns:meta:1.0", "meta"),
    ("urn:oasis:names:tc:opendocument:xmlns:manifest:1.0", "manifest"),
    ("http://www.w3.org/1999/xlink", "xlink"),
    ("urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0", "loext"),
];

/// An element: its name (`w:p`), attributes (`w:val`, `r:id`) and children.
#[derive(Clone, Debug, Default)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<Node>,
}

#[derive(Clone, Debug)]
pub enum Node {
    El(El),
    Text(String),
}

impl El {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// The name without its prefix.
    pub fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }

    pub fn is(&self, name: &str) -> bool {
        self.name == name
    }

    /// Child elements in order.
    pub fn elements(&self) -> impl Iterator<Item = &El> {
        self.kids.iter().filter_map(|n| match n {
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

    /// The first descendant (depth first) named `name`.
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

    /// Every descendant named `name`, depth first.
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a El>) {
        for e in self.elements() {
            if e.name == name {
                out.push(e);
            }
            e.find_all(name, out);
        }
    }

    /// The text directly inside (not in child elements).
    pub fn own_text(&self) -> String {
        let mut s = String::new();
        for k in &self.kids {
            if let Node::Text(t) = k {
                s.push_str(t);
            }
        }
        s
    }

    /// A child's `val` attribute (`<w:jc w:val="center"/>` → `child_val("w:jc", "w:val")`).
    pub fn child_attr(&self, child: &str, attr: &str) -> Option<&str> {
        self.child(child).and_then(|c| c.attr(attr))
    }
}

fn prefix_for(ns: &[u8]) -> Option<&'static str> {
    let ns = std::str::from_utf8(ns).ok()?;
    NAMESPACES.iter().find(|(u, _)| *u == ns).map(|(_, p)| *p)
}

fn qualified(res: ResolveResult, local: &[u8], raw: &[u8]) -> String {
    let local = String::from_utf8_lossy(local);
    match res {
        ResolveResult::Bound(ns) => match prefix_for(ns.0) {
            Some(p) => format!("{p}:{local}"),
            None => String::from_utf8_lossy(raw).into_owned(),
        },
        _ => String::from_utf8_lossy(raw).into_owned(),
    }
}

/// Parses a whole XML part into its root element.
pub fn parse(bytes: &[u8]) -> Result<El, String> {
    // Skip a byte order mark.
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let mut reader = NsReader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = false;
    let mut stack: Vec<El> = vec![El { name: "#root".into(), ..Default::default() }];
    let mut buf = Vec::new();
    loop {
        let (res, ev) = reader.read_resolved_event_into(&mut buf).map_err(|e| format!("The XML is damaged ({e})."))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(ev, Event::Empty(_));
                let name = qualified(res, e.local_name().as_ref(), e.name().as_ref());
                let mut attrs = Vec::new();
                for a in e.attributes().with_checks(false).flatten() {
                    let key = a.key;
                    if key.as_ref().starts_with(b"xmlns") {
                        continue;
                    }
                    let (ares, alocal) = reader.resolve_attribute(key);
                    let aname = match ares {
                        ResolveResult::Bound(ns) => match prefix_for(ns.0) {
                            Some(p) => format!("{p}:{}", String::from_utf8_lossy(alocal.as_ref())),
                            None => String::from_utf8_lossy(key.as_ref()).into_owned(),
                        },
                        _ => String::from_utf8_lossy(key.as_ref()).into_owned(),
                    };
                    let value = a.unescape_value().map(|v| v.into_owned()).unwrap_or_else(|_| String::from_utf8_lossy(&a.value).into_owned());
                    attrs.push((aname, value));
                }
                let el = El { name, attrs, kids: vec![] };
                if empty {
                    stack.last_mut().unwrap().kids.push(Node::El(el));
                } else {
                    stack.push(el);
                }
            }
            Event::End(_) => {
                if stack.len() > 1 {
                    let el = stack.pop().unwrap();
                    stack.last_mut().unwrap().kids.push(Node::El(el));
                }
            }
            Event::Text(t) => {
                let s = t.decode().map(|c| c.into_owned()).unwrap_or_default();
                push_text(stack.last_mut().unwrap(), &s);
            }
            Event::CData(t) => {
                let s = String::from_utf8_lossy(&t).into_owned();
                push_text(stack.last_mut().unwrap(), &s);
            }
            Event::GeneralRef(r) => {
                let s = match r.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    _ => match &*r as &[u8] {
                        b"amp" => "&".into(),
                        b"lt" => "<".into(),
                        b"gt" => ">".into(),
                        b"quot" => "\"".into(),
                        b"apos" => "'".into(),
                        _ => String::new(),
                    },
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
    let root = stack.pop().unwrap();
    root.kids.into_iter().find_map(|n| match n {
        Node::El(e) => Some(e),
        _ => None,
    })
    .ok_or_else(|| "The XML part is empty.".to_string())
}

fn push_text(el: &mut El, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Node::Text(t)) = el.kids.last_mut() {
        t.push_str(s);
    } else {
        el.kids.push(Node::Text(s.to_string()));
    }
}

// ---- zip ------------------------------------------------------------------------------------

/// A zip package being read.
pub struct Package {
    zip: zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    names: HashMap<String, String>,
}

impl Package {
    pub fn open(bytes: &[u8], what: &str) -> Result<Package, String> {
        let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).map_err(|_| format!("This isn't a {what} file (it isn't a zip package)."))?;
        // Part names are case-insensitive in OPC packages.
        let names = zip.file_names().map(|n| (n.trim_start_matches('/').to_ascii_lowercase(), n.to_string())).collect();
        Ok(Package { zip, names })
    }

    pub fn has(&self, name: &str) -> bool {
        self.names.contains_key(&name.trim_start_matches('/').to_ascii_lowercase())
    }

    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        let real = self.names.get(&name.trim_start_matches('/').to_ascii_lowercase())?.clone();
        let mut f = self.zip.by_name(&real).ok()?;
        // Refuse absurd sizes (zip bombs): 512 MB per part.
        if f.size() > 512 * 1024 * 1024 {
            return None;
        }
        let mut out = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut out).ok()?;
        Some(out)
    }

    pub fn xml(&mut self, name: &str) -> Option<El> {
        self.read(name).and_then(|b| parse(&b).ok())
    }
}

/// A zip package being written.
pub struct ZipOut {
    zip: zip::ZipWriter<std::io::Cursor<Vec<u8>>>,
}

impl ZipOut {
    pub fn new() -> Self {
        ZipOut { zip: zip::ZipWriter::new(std::io::Cursor::new(Vec::new())) }
    }

    fn options(stored: bool) -> zip::write::SimpleFileOptions {
        let o = zip::write::SimpleFileOptions::default()
            // A fixed date keeps exports reproducible.
            .last_modified_time(zip::DateTime::from_date_and_time(2026, 1, 1, 0, 0, 0).unwrap_or_default());
        if stored { o.compression_method(zip::CompressionMethod::Stored) } else { o.compression_method(zip::CompressionMethod::Deflated) }
    }

    pub fn add(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        self.add_with(name, bytes, false)
    }

    /// Stored without compression (ODF's `mimetype`, pictures already compressed).
    pub fn add_stored(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        self.add_with(name, bytes, true)
    }

    fn add_with(&mut self, name: &str, bytes: &[u8], stored: bool) -> Result<(), String> {
        self.zip.start_file(name, Self::options(stored)).map_err(|e| format!("Couldn't write {name}: {e}"))?;
        self.zip.write_all(bytes).map_err(|e| format!("Couldn't write {name}: {e}"))
    }

    pub fn finish(self) -> Result<Vec<u8>, String> {
        self.zip.finish().map(|c| c.into_inner()).map_err(|e| format!("Couldn't finish the file: {e}"))
    }
}

// ---- writing --------------------------------------------------------------------------------

/// Escapes text for element content and attribute values; drops characters XML 1.0 forbids.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// Base64 (standard alphabet, padded), for data URIs.
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// `#RRGGBB`, `RRGGBB` or `#rgb` to lowercase `#rrggbb`; `None` for `auto` and junk.
pub fn hex_color(s: &str) -> Option<String> {
    let h = s.trim().trim_start_matches('#');
    let full: String = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect() } else { h.to_string() };
    if full.len() == 6 && full.chars().all(|c| c.is_ascii_hexdigit()) { Some(format!("#{}", full.to_ascii_lowercase())) } else { None }
}

/// Font family by a font's name: `sans`, `serif`, `mono` or `display`.
pub fn family_of_font(name: &str) -> &'static str {
    let n = name.trim().trim_matches('\'').trim_matches('"').to_ascii_lowercase();
    if let Some(f) = folio_core::text::Family::parse(&n) {
        return match f {
            folio_core::text::Family::Sans => "sans",
            folio_core::text::Family::Serif => "serif",
            folio_core::text::Family::Mono => "mono",
            folio_core::text::Family::Display => "display",
        };
    }
    const MONO: &[&str] = &["mono", "courier", "consolas", "menlo", "monaco", "code", "lucida console", "inconsolata", "andale", "fixed", "terminal", "typewriter", "hack", "cascadia"];
    const SERIF: &[&str] = &[
        "times", "cambria", "georgia", "garamond", "palatino", "book antiqua", "baskerville", "minion", "merriweather", "constantia", "charter", "iowan", "hoefler", "didot", "bodoni", "century", "caslon", "bookman", "lora", "crimson", "playfair", "sabon", "perpetua", "goudy", "rockwell", "liberation serif", "dejavu serif", "noto serif", "pt serif", "source serif", "libre baskerville", "tinos", "new york", "athelas", "charis",
    ];
    if MONO.iter().any(|m| n.contains(m)) {
        return "mono";
    }
    if n.contains("sans") {
        return "sans";
    }
    if n.contains("serif") || SERIF.iter().any(|s| n.contains(s)) {
        return "serif";
    }
    if n.contains("chakra") {
        return "display";
    }
    "sans"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_are_canonical() {
        let x = br#"<?xml version="1.0"?><x:document xmlns:x="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:rr="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><x:body><x:p><x:r><x:t xml:space="preserve"> a &amp; b &#233;</x:t></x:r><x:hyperlink rr:id="rId4"/></x:p></x:body></x:document>"#;
        let root = parse(x).unwrap();
        assert_eq!(root.name, "w:document");
        let p = root.child("w:body").unwrap().child("w:p").unwrap();
        assert_eq!(p.find("w:t").unwrap().own_text(), " a & b é");
        assert_eq!(p.child("w:hyperlink").unwrap().attr("r:id"), Some("rId4"));
        assert_eq!(p.find("w:t").unwrap().attr("xml:space"), Some("preserve"));
    }

    #[test]
    fn base64_matches() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn fonts_map() {
        assert_eq!(family_of_font("Calibri"), "sans");
        assert_eq!(family_of_font("Times New Roman"), "serif");
        assert_eq!(family_of_font("Consolas"), "mono");
        assert_eq!(family_of_font("Liberation Sans"), "sans");
        assert_eq!(family_of_font("Noto Serif"), "serif");
        assert_eq!(family_of_font("Source Code Pro"), "mono");
    }
}
