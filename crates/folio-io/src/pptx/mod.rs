//! PowerPoint presentations (`.pptx`): every slide in and out.
//!
//! Reading ([`read`]) follows PowerPoint's inheritance: a placeholder without its own position
//! or text style takes its layout's, then its master's; colours go through the theme. Shapes
//! from the master and layout (logos, bars) are copied onto the slides that show them. Charts
//! keep their saved numbers on a "Charts data" sheet that the chart then reads.
//!
//! Writing ([`write`]) makes one master with folio's six layouts in the deck's theme, explicit
//! positions and text formatting on every shape (so every app shows the same thing), native
//! charts with their numbers in an embedded workbook, notes and hidden slides.

mod read;
mod write;

use folio_core::Document;

use crate::{Format, Imported};

pub const FORMAT: Format = Format {
    id: "pptx",
    name: "PowerPoint presentation",
    extensions: &["pptx"],
    kinds: &["deck"],
    import: true,
    export: true,
    apps: &["Microsoft PowerPoint", "Google Slides (File › Download › .pptx)", "Apple Keynote (File › Export To › PowerPoint)", "LibreOffice Impress"],
    notes: "Slides with text (sizes, styles, colours, fonts, bullets, alignment), shapes, pictures, tables, charts, backgrounds, speaker notes and hidden slides; the theme's colours and fonts. Charts come in reading a \"Charts data\" sheet and go out as PowerPoint charts with today's numbers. Animations, transitions, SmartArt, videos and embedded objects are left out; unusual shapes become rectangles.",
};

pub fn import(bytes: &[u8], title: &str) -> Result<Imported, String> {
    read::import(bytes, title)
}

pub fn export(doc: &Document, pages: &[usize]) -> Result<(Vec<u8>, Vec<String>), String> {
    write::export(doc, pages)
}

#[cfg(test)]
pub(crate) mod tests;
