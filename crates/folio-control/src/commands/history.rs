//! `history.*`: the one undo history every client shares.

use std::sync::Arc;

use serde_json::json;

use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "history.list" => s.read(|ed| {
            json!({
                "undo": ed.history().undo_list(),
                "redo": ed.history().redo_list(),
                "canUndo": ed.can_undo(),
                "canRedo": ed.can_redo(),
            })
        }),
        "history.undo" => {
            let step = s.with_editor(|ed| ed.undo())?.ok_or("Nothing to undo.")?;
            state(s, Some(step))
        }
        "history.redo" => {
            let step = s.with_editor(|ed| ed.redo())?.ok_or("Nothing to redo.")?;
            state(s, Some(step))
        }
        "history.checkpoint" => Ok(json!({ "checkpoint": s.read(|ed| ed.checkpoint())? })),
        "history.revertTo" => {
            let cp = a.opt_i64("checkpoint").unwrap_or(-1);
            if cp < 0 {
                return Err("checkpoint is a number from history.checkpoint".into());
            }
            let n = s.with_editor(|ed| ed.revert_to(cp as u64))?;
            let mut v = state(s, None)?;
            v["undone"] = json!(n);
            Ok(v)
        }
        _ => Err(super::unhandled(cx)),
    }
}

fn state(s: &Session, step: Option<folio_core::StepInfo>) -> CmdResult {
    s.read(|ed| json!({ "canUndo": ed.can_undo(), "canRedo": ed.can_redo(), "step": step.as_ref().map(|st| st.label.clone()), "source": step.as_ref().map(|st| st.source.clone()) }))
}
