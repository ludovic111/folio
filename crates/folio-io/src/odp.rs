//! OpenDocument presentation. Not written yet: see the module list in lib.rs.

use folio_core::Document;

use crate::{Format, Imported};

pub const FORMAT: Format = Format {
    id: "odp",
    name: "OpenDocument presentation",
    extensions: &["odp"],
    kinds: &["deck"],
    import: false,
    export: false,
    apps: &["LibreOffice Impress"],
    notes: "To be filled in.",
};

pub fn import(_bytes: &[u8], _title: &str) -> Result<Imported, String> {
    Err("Opening OpenDocument presentation files isn't ready yet.".into())
}

pub fn export(_doc: &Document, _pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    Err("Writing OpenDocument presentation files isn't ready yet.".into())
}
