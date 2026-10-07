//! lsuite AI as the agent's provider (lsuite's AI.md): the Anthropic Messages API served by the
//! lsuite server at `<server>/api/ai`, with the signed-in account's token as the key. Nothing to
//! install and no key to paste; the account is shared by every lsuite app
//! (`folio_control::account`).
//!
//! Its errors are the server's own lines: 402 `allowance_exhausted` and 403 `plan_required` /
//! `model_not_in_plan` end with "Manage plan: <url>". The agent never switches to another provider
//! by itself when lsuite AI refuses.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use folio_control::account;
use parking_lot::Mutex;
use serde_json::Value;

/// What the status says, and what `agent.send` answers, when nobody is signed in.
pub const SIGN_IN: &str = "Sign in to lsuite AI (no setup).";

/// The model when the plan names none (or the server can't be asked).
pub const FALLBACK_MODEL: &str = "claude-sonnet-5-5";

/// How long the plan's default model is remembered.
const TTL: Duration = Duration::from_secs(10 * 60);

/// One line for an error answer of the lsuite server (`{type: "error", error: {type, message…}}`):
/// [`account::error_line`], with Manage plan once even when the server's message already says it.
pub fn error_line(status: u16, text: &str) -> String {
    let body: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    if body["error"].is_null() {
        let detail = crate::tools::bounded(text.trim(), 300);
        return if detail.is_empty() { format!("lsuite AI answered {status}.") } else { format!("lsuite AI answered {status}: {detail}") };
    }
    dedupe_manage(account::error_line(status, &body))
}

/// `… Manage plan: url Manage plan: url` → `… Manage plan: url`.
fn dedupe_manage(line: String) -> String {
    const KEY: &str = "Manage plan:";
    match (line.find(KEY), line.rfind(KEY)) {
        (Some(first), Some(last)) if first != last => line[..last].trim_end().to_string(),
        _ => line,
    }
}

/// The "Manage plan" address in an error line, for a button next to it.
pub fn manage_url(line: &str) -> Option<String> {
    let rest = line.split("Manage plan:").nth(1)?;
    rest.split_whitespace().next().map(|u| u.trim_end_matches(['.', ',', ')']).to_string()).filter(|u| u.starts_with("http"))
}

type Cache = Mutex<HashMap<String, (Instant, String)>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The plan's default model (`GET <server>/api/account/me` → `defaultModel`), remembered a few
/// minutes; [`FALLBACK_MODEL`] when the server doesn't say.
pub async fn default_model(server: &str, token: &str) -> String {
    let key = format!("{server}|{}", fingerprint(token));
    if let Some((at, m)) = cache().lock().get(&key).cloned()
        && at.elapsed() < TTL
    {
        return m;
    }
    match account::me(server, token).await {
        Ok(me) => {
            let m = me["defaultModel"].as_str().filter(|m| !m.is_empty()).unwrap_or(FALLBACK_MODEL).to_string();
            cache().lock().insert(key, (Instant::now(), m.clone()));
            m
        }
        Err(e) => {
            tracing::debug!("lsuite AI: couldn't read the plan's default model: {e}");
            FALLBACK_MODEL.to_string()
        }
    }
}

/// A short fingerprint of a token, so the cache never holds the token itself.
fn fingerprint(token: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in token.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_one_line_with_manage_plan_once() {
        let body = r#"{"type":"error","error":{"type":"allowance_exhausted","message":"Your lsuite AI allowance for this month is used up (Pro, 4,000 credits). It resets on 1 Nov. Manage plan: http://x/account","manage_url":"http://x/account"}}"#;
        let line = error_line(402, body);
        assert_eq!(line.matches("Manage plan:").count(), 1, "{line}");
        assert!(line.starts_with("Your lsuite AI allowance") && !line.contains('\n'), "{line}");
        assert_eq!(manage_url(&line).as_deref(), Some("http://x/account"));
        let body = r#"{"type":"error","error":{"type":"plan_required","message":"lsuite AI needs a plan.","manage_url":"http://x/account"}}"#;
        assert_eq!(error_line(403, body), "lsuite AI needs a plan. Manage plan: http://x/account");
        assert_eq!(error_line(502, "Bad gateway"), "lsuite AI answered 502: Bad gateway");
        assert_eq!(manage_url("Nothing here."), None);
    }
}
