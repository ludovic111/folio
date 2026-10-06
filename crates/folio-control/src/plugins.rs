//! Plugins (see lsuite's PLUGINS.md). Placeholder until the plugin host lands.

use std::sync::Arc;

use crate::session::Session;

#[derive(Default)]
pub struct Host {}

impl Host {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scans the plugin folder and loads enabled plugins.
    pub fn load_all(&self, _session: &Arc<Session>) {}

    /// Registers every enabled plugin's spreadsheet functions with an engine.
    pub fn register_functions(&self, _engine: &mut folio_calc::Engine) {}
}
