//! `account.*`: lsuite AI, the suite's one account (see [`crate::account`]).

use std::sync::Arc;

use serde_json::json;

use crate::registry::{Args, Ctx};
use crate::session::{CmdResult, Event, Session};

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "account.status" => crate::account::status(a.bool_or("refresh", false)).await,
        "account.plans" => crate::account::plans().await,
        "account.signIn" => {
            if let Some(key) = a.opt_str("key") {
                let v = crate::account::sign_in_key(key).await?;
                s.emit(Event::AccountChanged);
                return Ok(v);
            }
            let started = crate::account::sign_in_browser(true).await?;
            let url = started.url.clone();
            let session = s.clone();
            let done = started.done;
            if a.bool_or("wait", false) {
                let v = done.await.map_err(|e| e.to_string())??;
                session.emit(Event::AccountChanged);
                return Ok(v);
            }
            tokio::spawn(async move {
                match done.await {
                    Ok(Ok(_)) => {
                        session.emit(Event::AccountChanged);
                        session.toast(crate::ToastKind::Success, "Signed in to lsuite AI.");
                    }
                    Ok(Err(e)) => session.toast(crate::ToastKind::Error, e),
                    Err(_) => {}
                }
            });
            Ok(json!({ "opened": url, "waiting": true, "note": "Finish in the browser; account.status shows the result." }))
        }
        "account.signOut" => {
            let v = crate::account::sign_out().await?;
            s.emit(Event::AccountChanged);
            Ok(v)
        }
        _ => Err(super::unhandled(cx)),
    }
}
