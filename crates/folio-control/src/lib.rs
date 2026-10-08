//! folio-control: the one door into folio.
//!
//! * [`registry`] names, validates and runs every command (`family.verb`).
//! * [`session::Session`] holds the open file and its single undo history, shared by the
//!   window, the built-in agent, `folio-cli` and `folio-mcp`.
//! * [`bridge`] lets the CLI and MCP drive the running app over a token-protected loopback
//!   socket; [`discovery`] tells other lsuite apps where folio is.
//! * [`account`] is lsuite AI's shared account; [`plugins`] loads plugins.

pub mod account;
pub mod bridge;
pub mod commands;
pub mod discovery;
pub mod harness;
pub mod overview;
pub mod plugins;
pub mod recent;
pub mod registry;
pub mod secrets;
pub mod session;
pub mod settings;
pub mod templates;
pub mod vision;

pub use registry::{Kind, Param, Perm, Spec, call, commands as specs, describe, input_schema, markdown, spec};
pub use session::{CmdResult, CommandRecord, Event, Session, SessionOptions, Source, ToastKind, UiCall, UiState};
pub use settings::Settings;

#[cfg(test)]
mod tests;

pub mod update;
