//! The whole file: pages of three kinds, the media they use, and a little metadata.

use std::collections::BTreeMap;
use std::sync::Arc;

use folio_calc::Axis;
use serde::{Deserialize, Serialize};

use crate::{Deck, Error, Id, Result, Sheet, TextDoc, bail};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageKind {
    Doc,
    Sheet,
    Deck,
}

impl PageKind {
    pub fn parse(s: &str) -> Option<PageKind> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "doc" | "document" | "text" | "word" | "page" => PageKind::Doc,
            "sheet" | "spreadsheet" | "grid" | "table" | "excel" => PageKind::Sheet,
            "deck" | "slides" | "presentation" | "slide" | "powerpoint" => PageKind::Deck,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            PageKind::Doc => "doc",
            PageKind::Sheet => "sheet",
            PageKind::Deck => "deck",
        }
    }

    /// What a new page of this kind is called ("Document 2").
    pub fn noun(self) -> &'static str {
        match self {
            PageKind::Doc => "Document",
            PageKind::Sheet => "Sheet",
            PageKind::Deck => "Deck",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PageBody {
    Doc(TextDoc),
    Sheet(Sheet),
    Deck(Deck),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub id: Id,
    pub name: String,
    #[serde(flatten)]
    pub body: PageBody,
}

impl Page {
    pub fn new(kind: PageKind, name: impl Into<String>) -> Self {
        let body = match kind {
            PageKind::Doc => PageBody::Doc(TextDoc::default()),
            PageKind::Sheet => PageBody::Sheet(Sheet::default()),
            PageKind::Deck => PageBody::Deck(Deck::default()),
        };
        Page { id: Id::new(), name: name.into(), body }
    }

    pub fn kind(&self) -> PageKind {
        match self.body {
            PageBody::Doc(_) => PageKind::Doc,
            PageBody::Sheet(_) => PageKind::Sheet,
            PageBody::Deck(_) => PageKind::Deck,
        }
    }

    pub fn doc(&self) -> Option<&TextDoc> {
        match &self.body {
            PageBody::Doc(d) => Some(d),
            _ => None,
        }
    }

    pub fn doc_mut(&mut self) -> Option<&mut TextDoc> {
        match &mut self.body {
            PageBody::Doc(d) => Some(d),
            _ => None,
        }
    }

    pub fn sheet(&self) -> Option<&Sheet> {
        match &self.body {
            PageBody::Sheet(s) => Some(s),
            _ => None,
        }
    }

    pub fn sheet_mut(&mut self) -> Option<&mut Sheet> {
        match &mut self.body {
            PageBody::Sheet(s) => Some(s),
            _ => None,
        }
    }

    pub fn deck(&self) -> Option<&Deck> {
        match &self.body {
            PageBody::Deck(d) => Some(d),
            _ => None,
        }
    }

    pub fn deck_mut(&mut self) -> Option<&mut Deck> {
        match &mut self.body {
            PageBody::Deck(d) => Some(d),
            _ => None,
        }
    }

    /// Plain text of the page (documents' text, sheets as tab-separated values, slides' text).
    pub fn plain(&self) -> String {
        match &self.body {
            PageBody::Doc(d) => d.plain(),
            PageBody::Sheet(s) => match s.used_range() {
                None => String::new(),
                Some(r) => s.grid(r).iter().map(|row| row.iter().map(|c| c.1.clone()).collect::<Vec<_>>().join("\t")).collect::<Vec<_>>().join("\n"),
            },
            PageBody::Deck(d) => d.plain(),
        }
    }
}

/// An image (or other file) the document holds. The bytes are in the `.folio` zip under
/// `media/<id>.<ext>`; in memory they are shared between undo snapshots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    pub id: Id,
    /// The file name it came from.
    pub name: String,
    pub mime: String,
    /// Pixel size, for images.
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(skip)]
    pub bytes: Arc<Vec<u8>>,
}

impl Media {
    /// The file extension for its type.
    pub fn ext(&self) -> &'static str {
        match self.mime.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/svg+xml" => "svg",
            "image/bmp" => "bmp",
            _ => "bin",
        }
    }

    pub fn zip_path(&self) -> String {
        format!("media/{}.{}", self.id, self.ext())
    }

    /// Guesses the type from the first bytes.
    pub fn sniff(bytes: &[u8]) -> &'static str {
        if bytes.starts_with(b"\x89PNG") {
            "image/png"
        } else if bytes.starts_with(&[0xff, 0xd8]) {
            "image/jpeg"
        } else if bytes.starts_with(b"GIF8") {
            "image/gif"
        } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            "image/webp"
        } else if bytes.starts_with(b"BM") {
            "image/bmp"
        } else if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") {
            "image/svg+xml"
        } else {
            "application/octet-stream"
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Meta {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub author: String,
    pub created: Option<chrono::DateTime<chrono::Utc>>,
    pub modified: Option<chrono::DateTime<chrono::Utc>>,
    /// The folio version that last wrote the file.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub generator: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub format: u32,
    pub id: Id,
    pub title: String,
    pub pages: Vec<Arc<Page>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub media: BTreeMap<Id, Media>,
    #[serde(default)]
    pub meta: Meta,
}

impl Default for Document {
    fn default() -> Self {
        Document::new("Untitled")
    }
}

impl Document {
    /// A new file with one empty document page.
    pub fn new(title: impl Into<String>) -> Self {
        Document {
            format: crate::FORMAT,
            id: Id::new(),
            title: title.into(),
            pages: vec![Arc::new(Page::new(crate::PageKind::Doc, "Document"))],
            media: BTreeMap::new(),
            meta: Meta { created: Some(chrono::Utc::now()), ..Default::default() },
        }
    }

    /// A new file with no pages (importers fill it).
    pub fn empty(title: impl Into<String>) -> Self {
        let mut d = Document::new(title);
        d.pages.clear();
        d
    }

    /// Whether `other` is the same document without looking inside unchanged pages.
    pub fn same_as(&self, other: &Document) -> bool {
        self.title == other.title
            && self.pages.len() == other.pages.len()
            && self.pages.iter().zip(&other.pages).all(|(a, b)| Arc::ptr_eq(a, b) || a == b)
            && self.media.len() == other.media.len()
            && self.media.keys().eq(other.media.keys())
            && self.meta.author == other.meta.author
    }

    /// A page by id, unique name (case-insensitive) or 1-based number.
    pub fn page_index(&self, key: &str) -> Option<usize> {
        let key = key.trim();
        self.pages
            .iter()
            .position(|p| p.id == key)
            .or_else(|| self.pages.iter().position(|p| p.name.eq_ignore_ascii_case(key)))
            .or_else(|| key.parse::<usize>().ok().filter(|n| *n >= 1 && *n <= self.pages.len()).map(|n| n - 1))
    }

    pub fn page(&self, key: &str) -> Result<&Page> {
        self.page_index(key).map(|i| &*self.pages[i]).ok_or_else(|| self.no_page(key))
    }

    fn no_page(&self, key: &str) -> Error {
        let names: Vec<String> = self.pages.iter().map(|p| format!("\"{}\" ({})", p.name, p.kind().id())).collect();
        Error(format!("No page \"{key}\". Pages: {}.", if names.is_empty() { "none".into() } else { names.join(", ") }))
    }

    /// The page at `i`, unshared so it can change (copy on write).
    pub fn page_mut(&mut self, i: usize) -> &mut Page {
        Arc::make_mut(&mut self.pages[i])
    }

    pub fn page_mut_by(&mut self, key: &str) -> Result<&mut Page> {
        let i = self.page_index(key).ok_or_else(|| self.no_page(key))?;
        Ok(self.page_mut(i))
    }

    /// A name no other page has: `base`, or `base 2`, `base 3`…
    pub fn unique_name(&self, base: &str) -> String {
        let taken = |n: &str| self.pages.iter().any(|p| p.name.eq_ignore_ascii_case(n));
        if !taken(base) {
            return base.to_string();
        }
        (2..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).unwrap()
    }

    /// Adds a page of `kind` at `at` (the end by default); returns its index.
    pub fn add_page(&mut self, kind: crate::PageKind, name: Option<&str>, at: Option<usize>) -> Result<usize> {
        let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => {
                if self.page_index(n).is_some_and(|i| self.pages[i].name.eq_ignore_ascii_case(n)) {
                    return bail(format!("A page is already called \"{n}\"."));
                }
                check_name(n)?;
                n.to_string()
            }
            None => {
                let count = self.pages.iter().filter(|p| p.kind() == kind).count();
                if count == 0 { self.unique_name(kind.noun()) } else { self.unique_name(&format!("{} {}", kind.noun(), count + 1)) }
            }
        };
        let at = at.unwrap_or(self.pages.len()).min(self.pages.len());
        self.pages.insert(at, Arc::new(Page::new(kind, self.unique_name(&name))));
        Ok(at)
    }

    /// Renames a page; formulas, links and charts that name it follow.
    pub fn rename_page(&mut self, i: usize, name: &str) -> Result<()> {
        let name = name.trim();
        check_name(name)?;
        if self.pages.iter().enumerate().any(|(j, p)| j != i && p.name.eq_ignore_ascii_case(name)) {
            return bail(format!("A page is already called \"{name}\"."));
        }
        let old = self.pages[i].name.clone();
        if old == name {
            return Ok(());
        }
        self.page_mut(i).name = name.to_string();
        if self.pages[i].kind() == crate::PageKind::Sheet {
            self.rewrite_refs(|f| folio_calc::rename_sheet(f, &old, name));
        }
        Ok(())
    }

    /// Rewrites every formula, link and chart source in the file (a sheet renamed, rows inserted).
    /// `f` takes formula text without `=`.
    pub fn rewrite_refs(&mut self, f: impl Fn(&str) -> String) {
        self.rewrite_refs_in(|_, formula| f(formula))
    }

    /// Like [`rewrite_refs`](Self::rewrite_refs), also telling `f` the name of the sheet a
    /// formula sits on (`None` for links and charts, which always name their sheet).
    pub fn rewrite_refs_in(&mut self, f: impl Fn(Option<&str>, &str) -> String) {
        for i in 0..self.pages.len() {
            let name = self.pages[i].name.clone();
            // Look before copying: most pages have nothing to rewrite.
            let changed = match &self.pages[i].body {
                PageBody::Sheet(s) => {
                    s.cells.iter().any(|(_, c)| c.is_formula() && f(Some(&name), &c.input[1..]) != c.input[1..]) || s.charts.iter().any(|c| f(None, &c.chart.source) != c.chart.source)
                }
                PageBody::Doc(d) => d.blocks.iter().any(|b| match b {
                    crate::Block::Table(t) => t.link.as_ref().is_some_and(|l| f(None, l) != *l),
                    crate::Block::Chart(c) => f(None, &c.chart.source) != c.chart.source,
                    _ => false,
                }),
                PageBody::Deck(d) => d.slides.iter().flat_map(|s| &s.shapes).any(|s| match &s.kind {
                    crate::ShapeKind::Chart { chart } => f(None, &chart.source) != chart.source,
                    crate::ShapeKind::Table { table } => table.link.as_ref().is_some_and(|l| f(None, l) != *l),
                    _ => false,
                }),
            };
            if !changed {
                continue;
            }
            match &mut self.page_mut(i).body {
                PageBody::Sheet(s) => {
                    let keys: Vec<_> = s.cells.iter().filter(|(_, c)| c.is_formula()).map(|(a, _)| *a).collect();
                    for a in keys {
                        if let Some(c) = s.cells.get_mut(&a) {
                            let nf = f(Some(&name), &c.input[1..]);
                            if nf != c.input[1..] {
                                c.input = format!("={nf}");
                            }
                        }
                    }
                    for c in &mut s.charts {
                        c.chart.source = f(None, &c.chart.source);
                    }
                }
                PageBody::Doc(d) => {
                    for bi in 0..d.blocks.len() {
                        match &mut d.blocks[bi] {
                            crate::Block::Table(t) => {
                                if let Some(l) = &mut t.link {
                                    *l = f(None, l);
                                }
                            }
                            crate::Block::Chart(c) => c.chart.source = f(None, &c.chart.source),
                            _ => {}
                        }
                    }
                }
                PageBody::Deck(d) => {
                    for s in d.slides.iter_mut().flat_map(|s| s.shapes.iter_mut()) {
                        match &mut s.kind {
                            crate::ShapeKind::Chart { chart } => chart.source = f(None, &chart.source),
                            crate::ShapeKind::Table { table } => {
                                if let Some(l) = &mut table.link {
                                    *l = f(None, l);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    /// Inserts (count > 0) or deletes (count < 0) rows or columns in a sheet page; every
    /// formula, link and chart in the file is rewritten to keep pointing at the same cells.
    pub fn insert_lines(&mut self, sheet: usize, rows: bool, at: u32, count: i64) -> Result<()> {
        if self.pages[sheet].kind() != crate::PageKind::Sheet {
            return bail(format!("\"{}\" isn't a sheet.", self.pages[sheet].name));
        }
        let target = self.pages[sheet].name.clone();
        let axis = |r: bool| if r { Axis::Rows } else { Axis::Cols };
        // Links and charts name their sheet; formulas on other sheets too, unqualified ones
        // belong to their own sheet.
        self.rewrite_refs_in(|own, formula| {
            let own_sheet = own.unwrap_or("\u{0}");
            folio_calc::adjust_for_insert(formula, own_sheet, &target, axis(rows), at, count)
        });
        if let Some(s) = self.page_mut(sheet).sheet_mut() {
            s.shift(rows, at, count);
        }
        Ok(())
    }

    /// Sheet pages in order: (page index, name).
    pub fn sheets(&self) -> Vec<(usize, &str)> {
        self.pages.iter().enumerate().filter(|(_, p)| p.kind() == crate::PageKind::Sheet).map(|(i, p)| (i, p.name.as_str())).collect()
    }

    /// Adds media from bytes and returns its id.
    pub fn add_media(&mut self, name: &str, bytes: Vec<u8>) -> Id {
        let mime = Media::sniff(&bytes).to_string();
        let (width, height) = image_size(&bytes).unwrap_or((0, 0));
        let id = Id::new();
        self.media.insert(id.clone(), Media { id: id.clone(), name: name.to_string(), mime, width, height, bytes: Arc::new(bytes) });
        id
    }

    /// Drops media no page uses any more.
    pub fn prune_media(&mut self) {
        let mut used = std::collections::HashSet::new();
        for p in &self.pages {
            match &p.body {
                PageBody::Doc(d) => {
                    for b in d.blocks.iter() {
                        if let crate::Block::Image(i) = b {
                            used.insert(i.media.clone());
                        }
                    }
                }
                PageBody::Deck(d) => {
                    for s in d.slides.iter().flat_map(|s| &s.shapes) {
                        if let crate::ShapeKind::Image { media } = &s.kind {
                            used.insert(media.clone());
                        }
                    }
                }
                PageBody::Sheet(_) => {}
            }
        }
        self.media.retain(|id, _| used.contains(id));
    }
}

/// Page names are also sheet names in formulas: no `!`, `'`, `[`, `]`, `:` and not empty.
pub fn check_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return bail("A page needs a name.");
    }
    if let Some(c) = name.chars().find(|c| matches!(c, '!' | '\'' | '[' | ']' | ':' | '*' | '?' | '/' | '\\')) {
        return bail(format!("Page names can't contain `{c}` (formulas use it)."));
    }
    if name.chars().count() > 80 {
        return bail("Page names are at most 80 characters.");
    }
    Ok(())
}

/// Width and height of a PNG, JPEG, GIF or WebP from its header.
pub fn image_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.starts_with(b"\x89PNG") && b.len() > 24 {
        return Some((u32::from_be_bytes(b[16..20].try_into().ok()?), u32::from_be_bytes(b[20..24].try_into().ok()?)));
    }
    if b.starts_with(b"GIF8") && b.len() > 10 {
        return Some((u16::from_le_bytes([b[6], b[7]]) as u32, u16::from_le_bytes([b[8], b[9]]) as u32));
    }
    if b.starts_with(&[0xff, 0xd8]) {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = b[i + 1];
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
                return Some((u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32, u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32));
            }
            i += 2 + len;
        }
        return None;
    }
    if b.len() > 30 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        match &b[12..16] {
            b"VP8X" => {
                let w = 1 + (b[24] as u32 | (b[25] as u32) << 8 | (b[26] as u32) << 16);
                let h = 1 + (b[27] as u32 | (b[28] as u32) << 8 | (b[29] as u32) << 16);
                return Some((w, h));
            }
            b"VP8 " => return Some(((u16::from_le_bytes([b[26], b[27]]) & 0x3fff) as u32, (u16::from_le_bytes([b[28], b[29]]) & 0x3fff) as u32)),
            b"VP8L" => {
                let bits = u32::from_le_bytes([b[21], b[22], b[23], b[24]]);
                return Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1));
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PageKind;

    #[test]
    fn page_json_is_flat() {
        let p = Page::new(PageKind::Sheet, "Budget");
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["kind"], "sheet");
        assert_eq!(v["name"], "Budget");
        let back: Page = serde_json::from_value(v).unwrap();
        assert_eq!(back.kind(), PageKind::Sheet);
    }

    #[test]
    fn names_are_unique() {
        let mut d = Document::new("t");
        let i = d.add_page(PageKind::Sheet, None, None).unwrap();
        let j = d.add_page(PageKind::Sheet, None, None).unwrap();
        assert_ne!(d.pages[i].name, d.pages[j].name);
        assert!(d.add_page(PageKind::Doc, Some("Document"), None).is_err());
        assert!(d.rename_page(i, "a!b").is_err());
    }

    #[test]
    fn rename_sheet_rewrites_formulas() {
        let mut d = Document::new("t");
        let s = d.add_page(PageKind::Sheet, Some("Data"), None).unwrap();
        let o = d.add_page(PageKind::Sheet, Some("Summary"), None).unwrap();
        d.page_mut(o).sheet_mut().unwrap().set_input(folio_calc::Addr::new(0, 0), "=SUM(Data!A1:A3)");
        d.rename_page(s, "Raw data").unwrap();
        assert_eq!(d.pages[o].sheet().unwrap().input(folio_calc::Addr::new(0, 0)), "=SUM('Raw data'!A1:A3)");
    }

    #[test]
    fn png_size() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(640u32.to_be_bytes());
        png.extend(480u32.to_be_bytes());
        png.extend([0; 8]);
        assert_eq!(image_size(&png), Some((640, 480)));
    }
}
