//! `app.*`: the app itself: version, commands, settings, keys, first-run setup, updates.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::registry::{self, Args, Ctx};
use crate::session::{CmdResult, Session};

/// Where folio's releases are published.
pub const RELEASES: &str = "https://api.github.com/repos/ludovic111/folio/releases/latest";
pub const PAGE: &str = "https://lsuite.xyz/folio";

pub async fn run(s: &Arc<Session>, cx: &Ctx, a: Args) -> CmdResult {
    match cx.spec.name {
        "app.info" => {
            let loc = s.location();
            Ok(json!({
                "app": "folio",
                "version": env!("CARGO_PKG_VERSION"),
                "dataDir": s.data_dir,
                "configDir": s.config_dir,
                "window": s.has_ui(),
                "bridgePort": s.bridge_port(),
                "headless": s.headless,
                "file": loc.map(|(p, untitled, _)| json!({ "path": p, "untitled": untitled })),
                "page": PAGE,
            }))
        }
        "app.commands" => match a.opt_str("command") {
            Some(name) => registry::spec(name).map(registry::describe).ok_or_else(|| format!("Unknown command `{name}`.")),
            None => Ok(json!(registry::commands().iter().map(registry::describe).collect::<Vec<_>>())),
        },
        "app.settings" => Ok(json!(s.settings())),
        "app.setSetting" => {
            let key = a.str("key")?.to_string();
            if cx.source.is_agent() && (key.starts_with("agent.permissions") || key == "agent.provider") {
                return Err(format!("`{key}` stays with the person: agents can't change it."));
            }
            let value = a.get("value").cloned().unwrap_or(Value::Null);
            let mut next = s.settings();
            next.set(&key, value)?;
            let saved = s.update_settings(|st| *st = next)?;
            Ok(json!({ "key": key, "value": saved.get(&key) }))
        }
        "app.setAgentKey" => {
            let provider = a.str("provider")?;
            if !matches!(provider, "anthropic" | "openai" | "openrouter" | "gemini" | "mistral") {
                return Err("provider is anthropic, openai, openrouter, gemini or mistral.".into());
            }
            s.set_secret(provider, a.opt_str("key"))?;
            s.emit(crate::session::Event::SettingsChanged);
            Ok(json!({ "provider": provider, "saved": a.opt_str("key").is_some_and(|k| !k.trim().is_empty()) }))
        }
        "app.onboarding" => Ok(onboarding(s)),
        "app.finishOnboarding" => {
            let from = a.opt_str("comingFrom").unwrap_or("").to_string();
            if !from.is_empty() && !matches!(from.as_str(), "office" | "google" | "apple" | "libreoffice" | "none") {
                return Err("comingFrom is office, google, apple, libreoffice or none.".into());
            }
            let provider = a.opt_str("provider").map(str::to_string);
            if let Some(p) = &provider
                && !crate::settings::AGENT_PROVIDERS.contains(&p.as_str())
            {
                return Err(format!("provider is one of {}.", crate::settings::AGENT_PROVIDERS.join(", ")));
            }
            if cx.source.is_agent() && provider.is_some() {
                return Err("The agent provider stays with the person.".into());
            }
            let st = s.update_settings(|st| {
                st.onboarding.completed = env!("CARGO_PKG_VERSION").into();
                st.onboarding.coming_from = from.clone();
                if let Some(v) = a.opt_bool("agent") {
                    st.agent.enabled = v;
                }
                if let Some(p) = &provider {
                    st.agent.provider = p.clone();
                }
                if let Some(au) = a.opt_str("author") {
                    st.editing.author = au.trim().to_string();
                }
            })?;
            Ok(json!({ "onboarding": st.onboarding, "agent": { "enabled": st.agent.enabled, "provider": st.agent.provider } }))
        }
        "app.checkUpdates" => check_updates().await,
        "app.quit" => s.ui_call("app.quit", json!({})).await,
        _ => Err(super::unhandled(cx)),
    }
}

/// The suites people come from, with the formats folio really opens from each (logo ids are
/// the window's `assets/logos/<id>.svg`).
pub fn coming_from() -> Value {
    json!([
        {
            "id": "office", "name": "Microsoft Office",
            "apps": [
                { "name": "Word", "logo": "word", "opens": ["docx"], "how": "Open the .docx." },
                { "name": "Excel", "logo": "excel", "opens": ["xlsx", "csv"], "how": "Open the .xlsx: formulas, formats and sheets come along." },
                { "name": "PowerPoint", "logo": "powerpoint", "opens": ["pptx"], "how": "Open the .pptx." },
            ],
        },
        {
            "id": "google", "name": "Google Workspace",
            "apps": [
                { "name": "Google Docs", "logo": "google-docs", "opens": ["docx"], "how": "File › Download › Microsoft Word (.docx), then open it." },
                { "name": "Google Sheets", "logo": "google-sheets", "opens": ["xlsx", "csv"], "how": "File › Download › Microsoft Excel (.xlsx), then open it." },
                { "name": "Google Slides", "logo": "google-slides", "opens": ["pptx"], "how": "File › Download › Microsoft PowerPoint (.pptx), then open it." },
            ],
        },
        {
            "id": "apple", "name": "Apple iWork",
            "apps": [
                { "name": "Pages", "logo": "pages", "opens": ["docx"], "how": "File › Export To › Word, then open the .docx (folio doesn't read .pages files)." },
                { "name": "Numbers", "logo": "numbers", "opens": ["xlsx", "csv"], "how": "File › Export To › Excel, then open the .xlsx (not .numbers files)." },
                { "name": "Keynote", "logo": "keynote", "opens": ["pptx"], "how": "File › Export To › PowerPoint, then open the .pptx (not .key files)." },
            ],
        },
        {
            "id": "libreoffice", "name": "LibreOffice",
            "apps": [
                { "name": "Writer", "logo": "libreoffice-writer", "opens": ["odt", "docx"], "how": "Open the .odt or .docx." },
                { "name": "Calc", "logo": "libreoffice-calc", "opens": ["ods", "xlsx", "csv"], "how": "Open the .ods or .xlsx." },
                { "name": "Impress", "logo": "libreoffice-impress", "opens": ["odp", "pptx"], "how": "Open the .odp or .pptx." },
            ],
        },
    ])
}

fn onboarding(s: &Session) -> Value {
    let st = s.settings();
    let account = crate::account::read();
    json!({
        "done": st.onboarding.is_done(),
        "comingFrom": st.onboarding.coming_from,
        "suites": coming_from(),
        "lsuiteAi": {
            "signedIn": account.is_some(),
            "email": account.as_ref().map(|a| a.email.clone()),
            "plan": account.as_ref().map(|a| a.plan.clone()),
            "pitch": "No setup. Sign in and your agent works.",
        },
        "providers": crate::settings::AGENT_PROVIDERS,
        "agent": { "enabled": st.agent.enabled, "provider": st.agent.provider },
        "author": st.author(),
    })
}

async fn check_updates() -> CmdResult {
    let current = env!("CARGO_PKG_VERSION");
    if std::env::var("FOLIO_NO_UPDATE").is_ok_and(|v| !v.is_empty() && v != "0") {
        return Ok(json!({ "current": current, "checked": false, "reason": "FOLIO_NO_UPDATE is set." }));
    }
    let client = reqwest::Client::builder().user_agent(format!("folio/{current}")).timeout(std::time::Duration::from_secs(10)).build().map_err(|e| e.to_string())?;
    let r = client.get(RELEASES).send().await.map_err(|e| format!("Couldn't reach GitHub: {e}"))?;
    if r.status().as_u16() == 404 {
        return Ok(json!({ "current": current, "latest": null, "newer": false, "note": "No release published yet." }));
    }
    let v: Value = r.json().await.map_err(|e| format!("GitHub's answer wasn't readable: {e}"))?;
    let tag = v["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string();
    let newer = match (semver::Version::parse(&tag), semver::Version::parse(current)) {
        (Ok(l), Ok(c)) => l > c,
        _ => false,
    };
    Ok(json!({ "current": current, "latest": tag, "newer": newer, "url": v["html_url"], "download": format!("{PAGE}/download") }))
}
