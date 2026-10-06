//! Excel workbook. Not written yet: see the module list in lib.rs.

use folio_core::Document;

use crate::{Format, Imported};

pub const FORMAT: Format = Format {
    id: "xlsx",
    name: "Excel workbook",
    extensions: &["xlsx"],
    kinds: &["sheet"],
    import: false,
    export: false,
    apps: &["Microsoft Excel", "Google Sheets (File › Download › .xlsx)", "Apple Numbers (File › Export To › Excel)", "LibreOffice Calc"],
    notes: "To be filled in.",
};

pub fn import(_bytes: &[u8], _title: &str) -> Result<Imported, String> {
    Err("Opening Excel workbook files isn't ready yet.".into())
}

pub fn export(_doc: &Document, _pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    Err("Writing Excel workbook files isn't ready yet.".into())
}
