//! `plugin.*`: placeholder until the plugin host lands.

use std::sync::Arc;

use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Session};

pub async fn run(_s: &Arc<Session>, cx: &Ctx, _a: Args) -> CmdResult {
    Err(format!("`{}` isn't ready yet.", cx.spec.name))
}
