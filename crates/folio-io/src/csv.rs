//! CSV. Not written yet: see the module list in lib.rs.

use folio_core::Document;

use crate::{Format, Imported};

pub const FORMAT: Format = Format {
    id: "csv",
    name: "CSV",
    extensions: &["csv", "tsv"],
    kinds: &["sheet"],
    import: false,
    export: false,
    apps: &["any spreadsheet"],
    notes: "To be filled in.",
};

pub fn import(_bytes: &[u8], _title: &str) -> Result<Imported, String> {
    Err("Opening CSV files isn't ready yet.".into())
}

pub fn export(_doc: &Document, _pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    Err("Writing CSV files isn't ready yet.".into())
}
