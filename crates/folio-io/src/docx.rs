//! Word documents (DOCX): what people bring from Microsoft Word, Google Docs, Apple Pages and
//! LibreOffice, and what they send back.
//!
//! In ([`read`]): paragraphs with their styles (Title, Subtitle, headings, quotes, captions, code,
//! followed through `basedOn` chains and outline levels), alignment, character formatting, links,
//! lists (from `numbering.xml`), tables, pictures, the page setup, header and footer text, comments
//! with replies, footnotes, tracked changes and field results. Out ([`write`]): the same, as a
//! package Word, LibreOffice, Google Docs and Pages open cleanly; each document page is a section.

mod read;
mod write;
pub(crate) mod xml;

#[cfg(test)]
pub(crate) mod tests;

use folio_core::Document;

use crate::{Format, Imported};

pub(crate) use read::{classify, heading_for_level, is_check_glyph, is_code_name, is_link_blue, page_name, parse_date};
pub(crate) use write::{face, sheet_table};

pub const FORMAT: Format = Format {
    id: "docx",
    name: "Word document",
    extensions: &["docx"],
    kinds: &["doc"],
    import: true,
    export: true,
    apps: &["Microsoft Word", "Google Docs (File › Download › .docx)", "Apple Pages (File › Export To › Word)", "LibreOffice Writer"],
    notes: "Text, styles, lists, tables, pictures, links, page setup, header and footer text, comments, footnotes and tracked changes come through. Opening: floating pictures are placed in line, merged cells are split, text boxes become paragraphs, Word charts, SmartArt, shapes, equations (as text) and pictures in metafile formats are left out, and only the first section's page setup is kept. Saving: charts become pictures, linked tables keep their current values, sheets become tables of their values, decks are left out.",
};

/// Reads a Word document as a new folio document with one page.
pub fn import(bytes: &[u8], title: &str) -> Result<Imported, String> {
    read::import(bytes, title)
}

/// Writes the chosen pages as a Word document: documents as sections, sheets as tables.
pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    write::export(doc, pages)
}
