//! folio-core: the document model.
//!
//! A folio file holds any mix of three kinds of page: **documents** (rich text: [`text`]),
//! **sheets** (a grid with formulas: [`sheet`], computed by `folio-calc`) and **decks**
//! (slides: [`deck`]). A table in a document, a chart on a slide or a table on a slide can be
//! **linked** to a sheet range ([`links`]), so the three kinds work as one.
//!
//! * [`Document`] is the whole file in memory. Pages are behind `Arc` and their big parts
//!   (blocks, cells) are persistent collections (`imbl`), so a copy of the whole document is
//!   cheap: that is what makes one undo history ([`history`]) with snapshots fast.
//! * [`Editor`] owns the open document, its undo history and the formula engine, and is the
//!   only way to change it (`folio-control` wraps it in commands).
//! * [`file`] reads and writes `.folio` files (a zip of JSON and media, see
//!   `docs/FILE_FORMAT.md`).

pub mod chart;
pub mod deck;
pub mod doc;
pub mod editor;
pub mod file;
pub mod history;
pub mod id;
pub mod links;
pub mod recalc;
pub mod sheet;
pub mod text;

pub use chart::{Chart, ChartData, ChartKind, Series};
pub use deck::{Deck, DeckTheme, Shape, ShapeKind, Slide, SlideLayout};
pub use doc::{Document, Media, Meta, Page, PageBody, PageKind};
pub use editor::Editor;
pub use history::{History, StepInfo};
pub use id::Id;
pub use sheet::{Cell, CellFormat, Sheet};
pub use text::{Align, Block, ListKind, ParaStyle, Paragraph, Pos, Run, RunStyle, TextDoc};

/// Errors from model operations, written to be shown as they are.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Shorthand for an [`Error`] result.
pub fn bail<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error(msg.into()))
}

/// The file format version this build writes (`format` in `document.json`).
pub const FORMAT: u32 = 1;
