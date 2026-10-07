//! OpenDocument text (ODT): LibreOffice Writer's own format.
//!
//! In ([`read`]): paragraphs and headings with their styles (named and automatic), character
//! formatting, links, nested lists, tables, pictures in `Pictures/`, page breaks, the page
//! setup with header and footer text, footnotes, comments and tracked changes. Out ([`write`]):
//! the same, as a package with `mimetype` stored first, a manifest, content, styles and meta.

mod read;
mod write;

#[cfg(test)]
mod tests;

use folio_core::Document;

use crate::{Format, Imported};

pub const FORMAT: Format = Format {
    id: "odt",
    name: "OpenDocument text",
    extensions: &["odt"],
    kinds: &["doc"],
    import: true,
    export: true,
    apps: &["LibreOffice Writer", "Collabora Online", "Microsoft Word", "Google Docs"],
    notes: "Text, styles, lists, tables, pictures, links, page setup, header and footer text, footnotes, comments and tracked changes come through. Opening: floating pictures are placed in line, merged cells are split, drawn shapes and embedded objects are left out, and only the default page style is kept. Saving: comment replies become lines of their comment, charts become pictures, linked tables keep their current values, sheets become tables of their values, decks are left out.",
};

/// Reads an OpenDocument text file as a new folio document with one page.
pub fn import(bytes: &[u8], title: &str) -> Result<Imported, String> {
    read::import(bytes, title)
}

/// Writes the chosen pages as OpenDocument text: documents with their page styles, sheets as
/// tables.
pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    write::export(doc, pages)
}
