//! `ui.*`: what the window shows. `ui.state` and `ui.theme` work without the window (headless
//! sessions keep a state of their own); the others are carried out by the window.

use std::sync::Arc;

use serde_json::json;

use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "ui.state" => Ok(json!(s.ui_state())),
        "ui.theme" => {
            let mode = a.str("mode")?.to_string();
            if !matches!(mode.as_str(), "dark" | "light" | "system") {
                return Err("mode is dark, light or system.".into());
            }
            s.update_settings(|st| st.appearance.mode = mode.clone())?;
            Ok(json!({ "mode": mode }))
        }
        name => s.ui_call(name, serde_json::Value::Object(a.0)).await,
    }
}
