//! folio-io: other formats in and out.
//!
//! Each format is a module with a [`Format`] description, and `import` / `export` functions
//! where it supports them. Importing gives a new [`Document`] and the list of what didn't come
//! through; exporting writes the pages the format can carry (documents to word processors,
//! sheets to spreadsheets, decks to presentations; PDF and HTML take every kind) and says what it
//! left out.

pub mod csv;
pub mod docx;
pub mod html;
pub mod markdown;
pub mod odp;
pub mod ods;
pub mod odt;
pub mod pdf;
pub mod pptx;
pub mod xlsx;

use std::path::Path;

use folio_core::Document;
use serde::Serialize;

/// One format folio reads or writes.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Format {
    pub id: &'static str,
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    /// What kinds of page it carries: doc, sheet, deck.
    pub kinds: &'static [&'static str],
    pub import: bool,
    pub export: bool,
    /// Apps whose files these are (and how they get there).
    pub apps: &'static [&'static str],
    /// What doesn't survive the trip.
    pub notes: &'static str,
}

/// What an import gives: the document, the format it was, what didn't come through.
#[derive(Debug)]
pub struct Imported {
    pub doc: Document,
    pub warnings: Vec<String>,
    pub format: &'static str,
}

/// What an export wrote and what it couldn't carry.
pub struct Report {
    pub format: &'static str,
    pub warnings: Vec<String>,
}

/// Every format, in the order the Open and Export dialogs list them.
pub fn formats() -> Vec<Format> {
    vec![
        docx::FORMAT,
        xlsx::FORMAT,
        pptx::FORMAT,
        pdf::FORMAT,
        odt::FORMAT,
        ods::FORMAT,
        odp::FORMAT,
        csv::FORMAT,
        markdown_format(),
        html::FORMAT,
    ]
}

fn markdown_format() -> Format {
    Format {
        id: "md",
        name: "Markdown",
        extensions: &["md", "markdown"],
        kinds: &["doc"],
        import: true,
        export: true,
        apps: &["Obsidian", "GitHub", "any text editor"],
        notes: "Text, headings, lists, tables and links; no page setup, colours, pictures or comments.",
    }
}

/// The format a file name says, among those folio knows.
pub fn format_for_path(path: &Path) -> Option<Format> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    formats().into_iter().find(|f| f.extensions.contains(&ext.as_str()))
}

fn title_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into())
}

/// Reads a file in another format as a new document.
pub fn import(path: &Path) -> Result<Imported, String> {
    let f = format_for_path(path).ok_or_else(|| format!("folio can't open .{} files.", path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()))?;
    if !f.import {
        return Err(format!("folio writes {} files but can't open them.", f.name));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    import_bytes(&bytes, f.id, &title_of(path))
}

/// Reads bytes in a format (`docx`, `xlsx`…) as a new document titled `title`.
pub fn import_bytes(bytes: &[u8], format: &str, title: &str) -> Result<Imported, String> {
    match format {
        "docx" => docx::import(bytes, title),
        "xlsx" => xlsx::import(bytes, title),
        "pptx" => pptx::import(bytes, title),
        "odt" => odt::import(bytes, title),
        "ods" => ods::import(bytes, title),
        "odp" => odp::import(bytes, title),
        "csv" => csv::import(bytes, title),
        "md" => {
            let text = String::from_utf8_lossy(bytes);
            let mut doc = Document::empty(title);
            let i = doc.add_page(folio_core::PageKind::Doc, Some(title), None).map_err(|e| e.0)?;
            let t = doc.page_mut(i).doc_mut().unwrap();
            t.blocks = markdown::to_blocks(&text).into_iter().collect();
            folio_core::text::ensure_nonempty(&mut t.blocks);
            Ok(Imported { doc, warnings: vec![], format: "md" })
        }
        other => Err(format!("folio can't open {other} files.")),
    }
}

/// Writes `doc` (or some of its pages, by index) to `path` in `format` (or the format its
/// extension says).
pub fn export(doc: &Document, path: &Path, format: Option<&str>, pages: Option<&[usize]>) -> Result<Report, String> {
    let id = match format {
        Some(f) => f.trim().trim_start_matches('.').to_ascii_lowercase(),
        None => format_for_path(path).map(|f| f.id.to_string()).ok_or_else(|| format!("folio doesn't know the .{} format; say format=pdf, docx, xlsx, pptx, odt, ods, odp, csv, md or html.", path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()))?,
    };
    let (bytes, report) = export_bytes(doc, &id, pages)?;
    folio_core::file::write_atomic(path, &bytes).map_err(|e| e.0)?;
    Ok(report)
}

/// The bytes of an export.
pub fn export_bytes(doc: &Document, format: &str, pages: Option<&[usize]>) -> Result<(Vec<u8>, Report), String> {
    let selected: Vec<usize> = match pages {
        Some(p) => p.to_vec(),
        None => (0..doc.pages.len()).collect(),
    };
    let (bytes, warnings, id) = match format {
        "docx" => {
            let (b, w) = docx::export(doc, &selected)?;
            (b, w, "docx")
        }
        "xlsx" => {
            let (b, w) = xlsx::export(doc, &selected)?;
            (b, w, "xlsx")
        }
        "pptx" => {
            let (b, w) = pptx::export(doc, &selected)?;
            (b, w, "pptx")
        }
        "pdf" => {
            let (b, w) = pdf::export(doc, &selected)?;
            (b, w, "pdf")
        }
        "odt" => {
            let (b, w) = odt::export(doc, &selected)?;
            (b, w, "odt")
        }
        "ods" => {
            let (b, w) = ods::export(doc, &selected)?;
            (b, w, "ods")
        }
        "odp" => {
            let (b, w) = odp::export(doc, &selected)?;
            (b, w, "odp")
        }
        "csv" => {
            let (b, w) = csv::export(doc, &selected)?;
            (b, w, "csv")
        }
        "html" | "htm" => {
            let (b, w) = html::export(doc, &selected)?;
            (b, w, "html")
        }
        "md" | "markdown" => {
            let mut out = String::new();
            let mut warnings = vec![];
            for &i in &selected {
                let p = &doc.pages[i];
                match &p.body {
                    folio_core::PageBody::Doc(t) => {
                        if !out.is_empty() {
                            out.push_str("\n---\n\n");
                        }
                        out.push_str(&markdown::from_blocks(doc, &t.blocks));
                    }
                    _ => warnings.push(format!("\"{}\" is a {} and Markdown only carries documents: left out.", p.name, p.kind().id())),
                }
            }
            (out.into_bytes(), warnings, "md")
        }
        other => return Err(format!("folio can't write {other}: pdf, docx, xlsx, pptx, odt, ods, odp, csv, md or html.")),
    };
    Ok((bytes, Report { format: id, warnings }))
}

/// Pages of `kind` among `selected`, and warnings for the others.
pub fn pages_of_kind(doc: &Document, selected: &[usize], kind: folio_core::PageKind, format: &str) -> (Vec<usize>, Vec<String>) {
    let mut keep = vec![];
    let mut warnings = vec![];
    for &i in selected {
        let p = &doc.pages[i];
        if p.kind() == kind {
            keep.push(i);
        } else {
            warnings.push(format!("\"{}\" is a {} and {format} only carries {}s: left out (export it as {} or PDF).", p.name, p.kind().id(), kind.id(), match p.kind() {
                folio_core::PageKind::Doc => "DOCX",
                folio_core::PageKind::Sheet => "XLSX",
                folio_core::PageKind::Deck => "PPTX",
            }));
        }
    }
    (keep, warnings)
}
