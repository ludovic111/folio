//! Word document. Not written yet: see the module list in lib.rs.

use folio_core::Document;

use crate::{Format, Imported};

pub const FORMAT: Format = Format {
    id: "docx",
    name: "Word document",
    extensions: &["docx"],
    kinds: &["doc"],
    import: false,
    export: false,
    apps: &["Microsoft Word", "Google Docs (File › Download › .docx)", "Apple Pages (File › Export To › Word)", "LibreOffice Writer"],
    notes: "To be filled in.",
};

pub fn import(_bytes: &[u8], _title: &str) -> Result<Imported, String> {
    Err("Opening Word document files isn't ready yet.".into())
}

pub fn export(_doc: &Document, _pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    Err("Writing Word document files isn't ready yet.".into())
}
