//! Which providers the panel can use right now, each with a sentence for the person and the one
//! thing to do next when it can't be used yet.
//!
//! Local checks only: a CLI found and signed in, a key present, a local server answering. No
//! model request is sent; the first message is what proves model access.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use folio_control::Session;
use serde::Serialize;
use serde_json::Value;
use tokio::process::Command;

use crate::cli::{cli_executable, mcp_executable};
use crate::providers::Group;
use crate::{AgentConfig, KeySource, ProviderKind, key_for};

/// What stands between a provider and a first message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Next {
    /// Sign in to lsuite AI.
    Account,
    /// Pick an lsuite AI plan (the account is on Free), or manage it.
    Plan,
    /// Paste an API key.
    Key,
    /// Install the CLI or the app.
    Install,
    /// Sign in to the CLI.
    SignIn,
    /// Start the local server (or the app's server).
    Start,
    /// Give the address: a server's URL.
    Address,
    /// Choose a model, or get one (Ollama has none).
    Model,
    /// Restart or reinstall folio (its MCP bridge is missing).
    Restart,
}

/// A button for the next thing to do: a link to open, a command to run in a terminal, or a folio
/// command to run (`account.signIn`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// A folio command that does it (`account.signIn`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folio: Option<String>,
}

impl Action {
    fn link(label: &str, url: &str) -> Option<Self> {
        Some(Self { label: label.into(), url: Some(url.into()), command: None, folio: None })
    }

    fn run(label: &str, command: &str) -> Option<Self> {
        Some(Self { label: label.into(), url: None, command: Some(command.into()), folio: None })
    }

    fn folio(label: &str, command: &str) -> Option<Self> {
        Some(Self { label: label.into(), url: None, command: None, folio: Some(command.into()) })
    }
}

/// A provider's key: whether there is one, from where, and how to get one.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    /// A key is needed (else optional).
    pub required: bool,
    /// One is saved in the keychain.
    pub saved: bool,
    /// Where the key in use comes from: `keychain` or an environment variable.
    pub source: Option<String>,
    /// Environment variables read when nothing is saved.
    pub env: Vec<&'static str>,
    /// Where to get one.
    pub url: Option<&'static str>,
    /// What one looks like.
    pub hint: &'static str,
    /// It can be saved with `app.setAgentKey` (else only read from the environment).
    pub savable: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub provider: ProviderKind,
    pub label: &'static str,
    /// `lsuite`, `cli` (on this computer), `api` (model APIs) or `local` (local servers).
    pub group: Group,
    pub group_label: &'static str,
    /// One plain line on what it is.
    pub tagline: &'static str,
    /// It can be used now.
    pub ready: bool,
    /// The one chosen in `settings.agent.provider`.
    pub active: bool,
    /// What to tell the person ("Claude Code 2.1 is installed and signed in.").
    pub message: String,
    /// lsuite AI, signed in: "Pro · 38 % used · resets 1 Nov".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// lsuite AI: where the plan is managed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manage_url: Option<String>,
    /// What to do next when it isn't ready.
    pub next: Option<Next>,
    /// The button for it.
    pub action: Option<Action>,
    /// The executable or address that was checked.
    pub detail: String,
    /// Model used when `settings.agent.model` is empty (empty: the provider decides).
    pub default_model: String,
    /// Models a local server has (Ollama, LM Studio); `agent.models` lists anyone's.
    pub models: Vec<String>,
    /// The key, for the providers that take one.
    pub key: Option<KeyStatus>,
    /// The address used (the setting, else the default).
    pub base_url: String,
    pub default_base_url: &'static str,
    /// The address is the person's to give (a custom server).
    pub needs_base_url: bool,
    /// What the address field takes.
    pub base_url_hint: &'static str,
    /// Nothing leaves this computer.
    pub on_device: bool,
    pub website: &'static str,
}

/// Every provider with whether it is usable, for the panel, its settings and the first-run setup.
pub async fn provider_status(session: &Arc<Session>) -> Vec<ProviderStatus> {
    let checks = ProviderKind::ALL.iter().map(|&kind| status_of(session, kind));
    futures::future::join_all(checks).await
}

/// One provider's status.
pub async fn status_of(session: &Arc<Session>, kind: ProviderKind) -> ProviderStatus {
    let settings = session.settings().agent;
    let active = AgentConfig::from_settings(&settings).provider;
    let mut config = AgentConfig::from_settings(&settings);
    if config.provider != kind {
        config = AgentConfig::new(kind);
    }
    let info = kind.info();
    let key = key_status(session, kind);
    let mut s = ProviderStatus {
        provider: kind,
        label: info.label,
        group: info.group,
        group_label: info.group.label(),
        tagline: info.tagline,
        ready: false,
        active: kind == active,
        message: String::new(),
        summary: None,
        manage_url: None,
        next: None,
        action: None,
        detail: config.base_url(),
        default_model: kind.default_model().to_string(),
        models: vec![],
        key: key.clone(),
        base_url: config.base_url(),
        default_base_url: info.default_base_url,
        needs_base_url: info.needs_base_url,
        base_url_hint: info.base_url_hint,
        on_device: info.group == Group::Local && kind != ProviderKind::OpenAiCompatible,
        website: info.website,
    };
    match kind {
        ProviderKind::Lsuite => lsuite_status(&mut s).await,
        ProviderKind::ClaudeCode | ProviderKind::Codex => cli_status(session, kind, &mut s).await,
        ProviderKind::Ollama => ollama_status(&config, &mut s).await,
        ProviderKind::LmStudio => lmstudio_status(session, &config, &mut s).await,
        ProviderKind::OpenAiCompatible => compatible_status(session, &config, &mut s).await,
        _ => key_provider_status(&config, &key, &mut s),
    }
    // Every provider names its models, for the model pickers: as last fetched, else built in.
    if s.models.is_empty() {
        s.models = crate::models::known(kind).0.into_iter().map(|m| m.id).collect();
    }
    s
}

fn key_status(session: &Session, kind: ProviderKind) -> Option<KeyStatus> {
    let spec = kind.info().key?;
    let found = key_for(session, kind);
    let source = found.map(|(_, src)| match src {
        KeySource::Keychain => "keychain".to_string(),
        KeySource::Env(var) => var.to_string(),
        KeySource::Account => "account".to_string(),
    });
    let savable = crate::status::SAVABLE_KEYS.contains(&spec.id);
    Some(KeyStatus { required: spec.required, saved: session.secret(spec.id).is_some(), source, env: spec.env.to_vec(), url: spec.url, hint: spec.hint, savable })
}

/// The key ids `app.setAgentKey` saves in the keychain.
pub(crate) const SAVABLE_KEYS: &[&str] = &["anthropic", "openai", "openrouter", "gemini", "mistral"];

/// lsuite AI: signed in or not, the plan and the allowance (`GET /api/account/me`).
async fn lsuite_status(s: &mut ProviderStatus) {
    use folio_control::account;
    s.base_url = account::api_base();
    s.detail = s.base_url.clone();
    s.manage_url = Some(format!("{}/account", account::server()));
    let Some(acc) = account::read() else {
        s.message = crate::lsuite::SIGN_IN.into();
        s.next = Some(Next::Account);
        s.action = Action::folio("Sign in", "account.signIn");
        return;
    };
    s.detail = acc.email.clone();
    match account::me(&account::server(), &acc.token).await {
        Ok(me) => {
            let summary = account::summary(&me);
            if let Some(u) = me["manageUrl"].as_str() {
                s.manage_url = Some(u.to_string());
            }
            if let Some(m) = me["defaultModel"].as_str().filter(|m| !m.is_empty()) {
                s.default_model = m.to_string();
            }
            s.models = me["models"].as_array().into_iter().flatten().filter_map(|m| m.as_str().or_else(|| m["id"].as_str())).map(str::to_string).collect();
            if me["plan"].as_str().is_none_or(|p| p == "free") {
                s.message = format!("Signed in as {}, on Free: lsuite AI needs a plan (bring your own provider stays free).", acc.email);
                s.next = Some(Next::Plan);
                s.action = Action::link("Manage plan", s.manage_url.as_deref().unwrap_or(""));
            } else {
                s.ready = true;
                s.message = format!("Signed in as {}. {summary}.", acc.email);
            }
            s.summary = Some(summary);
        }
        Err(e) => {
            // Signed in but the server didn't answer: a message may still go through (or say why).
            s.ready = true;
            s.message = format!("Signed in as {}; lsuite didn't answer just now ({e}).", acc.email);
            s.summary = (!acc.plan.is_empty()).then(|| acc.plan.clone());
        }
    }
}

/// `console.mistral.ai/api-keys` for `https://console.mistral.ai/api-keys`.
fn short_url(url: &str) -> &str {
    url.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.")
}

fn key_provider_status(config: &AgentConfig, key: &Option<KeyStatus>, s: &mut ProviderStatus) {
    let kind = config.provider;
    let info = kind.info();
    let spec = info.key.expect("an API provider has a key");
    let source = key.as_ref().and_then(|k| k.source.clone());
    match source.as_deref() {
        None => {
            let env = spec.env.first().map(|e| format!(", or set {e}")).unwrap_or_default();
            let url = spec.url.unwrap_or(info.website);
            s.message = format!("No key yet: get one at {} and paste it in Settings › Agent{env}. It is billed by {} per use.", short_url(url), info.label.trim_end_matches(" API"));
            s.next = Some(Next::Key);
            s.action = Action::link("Get a key", url);
        }
        Some("keychain") => {
            s.ready = true;
            s.message = "Ready: a key is saved in the keychain.".into();
        }
        Some(var) => {
            s.ready = true;
            s.message = format!("Ready: using {var} from the environment.");
        }
    }
    if kind == ProviderKind::OpenAi && s.base_url != info.default_base_url && !s.ready {
        s.ready = true;
        s.next = None;
        s.action = None;
        s.message = format!("Ready to try the server at {} (no key).", s.base_url);
    }
}

async fn ollama_status(config: &AgentConfig, s: &mut ProviderStatus) {
    let base = config.base_url();
    match crate::models::ollama(&crate::http::client(), &base).await {
        Ok(models) if models.is_empty() => {
            s.message = "Ollama is running but has no models. Get one that can use tools: `ollama pull qwen3`.".into();
            s.next = Some(Next::Model);
            s.action = Action::run("Copy the command", "ollama pull qwen3");
        }
        Ok(models) => {
            let tools = models.iter().filter(|m| m.tools != Some(false)).count();
            s.ready = tools > 0;
            s.models = models.iter().map(|m| m.id.clone()).collect();
            s.message = if tools == 0 {
                s.next = Some(Next::Model);
                s.action = Action::run("Copy the command", "ollama pull qwen3");
                "None of Ollama's models can use tools. Get one that can: `ollama pull qwen3`.".into()
            } else {
                format!("Ollama is running with {} model{}. Nothing leaves this computer.", models.len(), if models.len() == 1 { "" } else { "s" })
            };
        }
        Err(_) => {
            s.message = format!("Ollama isn't running on {}. Start it, or install it from ollama.com.", base.trim_start_matches("http://"));
            s.next = Some(Next::Start);
            s.action = Action::link("Get Ollama", "https://ollama.com/download");
        }
    }
}

async fn lmstudio_status(session: &Session, config: &AgentConfig, s: &mut ProviderStatus) {
    let base = config.base_url();
    let key = key_for(session, ProviderKind::LmStudio).map(|(k, _)| k);
    match crate::models::fetch(&crate::http::client(), ProviderKind::LmStudio, &base, key.as_deref()).await {
        Ok(models) if models.is_empty() => {
            s.message = "LM Studio is running but has no models. Download one that can use tools (Qwen3, gpt-oss) in LM Studio.".into();
            s.next = Some(Next::Model);
        }
        Ok(models) => {
            s.models = models.iter().map(|m| m.id.clone()).collect();
            let loaded = models.iter().filter(|m| m.loaded == Some(true)).count();
            s.ready = true;
            s.message = if loaded == 0 && models.iter().any(|m| m.loaded.is_some()) {
                format!("LM Studio is running with {} models; it loads the one you choose. Nothing leaves this computer.", models.len())
            } else {
                format!("LM Studio is running with {} model{}. Nothing leaves this computer.", models.len(), if models.len() == 1 { "" } else { "s" })
            };
        }
        Err(e) if e.contains("401") || e.contains("403") => {
            s.message = "LM Studio asks for an API token: paste the one from its server settings.".into();
            s.next = Some(Next::Key);
        }
        Err(_) => {
            s.message = format!("LM Studio isn't running on {}. Open LM Studio and start its server (Developer › Start server).", base.trim_start_matches("http://").trim_end_matches("/v1"));
            s.next = Some(Next::Start);
            s.action = Action::link("Get LM Studio", "https://lmstudio.ai/download");
        }
    }
}

async fn compatible_status(session: &Session, config: &AgentConfig, s: &mut ProviderStatus) {
    if config.base_url.trim().is_empty() {
        s.message = "Give the server's address (for example http://127.0.0.1:8000/v1), a key if it needs one, and a model.".into();
        s.next = Some(Next::Address);
        return;
    }
    let base = config.base_url();
    let key = key_for(session, ProviderKind::OpenAiCompatible).map(|(k, _)| k);
    match crate::models::fetch(&crate::http::client(), ProviderKind::OpenAiCompatible, &base, key.as_deref()).await {
        Ok(models) => {
            s.models = models.iter().map(|m| m.id.clone()).collect();
            s.ready = true;
            s.message = format!("The server at {base} is answering.");
        }
        Err(e) if e.contains("401") || e.contains("403") => {
            s.message = format!("The server at {base} asks for a key.");
            s.next = Some(Next::Key);
        }
        // Some servers have no model list; a chosen model may still work.
        Err(_) if !config.model.trim().is_empty() => {
            s.ready = true;
            s.message = format!("Ready to try {} on {base}.", config.model.trim());
        }
        Err(_) => {
            s.message = format!("Nothing answers at {base}. Start the server, or check the address.");
            s.next = Some(Next::Start);
        }
    }
}

/// Runs `exe args` for a short check; `None` if it can't start or takes too long.
async fn probe(exe: &Path, args: &[&str]) -> Option<(bool, String)> {
    let mut cmd = Command::new(exe);
    // The same PATH as a run, so an npm script finds `node` from the Dock too.
    cmd.args(args).env("PATH", crate::cli::child_path(exe)).stdin(std::process::Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up on each check
    let out = tokio::time::timeout(Duration::from_secs(8), cmd.output()).await.ok()?.ok()?;
    Some((out.status.success(), String::from_utf8_lossy(&out.stdout).trim().to_string()))
}

async fn cli_status(session: &Session, kind: ProviderKind, s: &mut ProviderStatus) {
    let label = kind.label();
    s.detail = String::new();
    s.base_url = String::new();
    let Some(exe) = cli_executable(kind) else {
        let (how, action) = match kind {
            ProviderKind::Codex => ("Install it (npm install -g @openai/codex) and sign in with `codex login`.", Action::run("Copy the install command", "npm install -g @openai/codex")),
            _ => ("Install it from claude.com/claude-code and sign in by running `claude` once.", Action::link("Get Claude Code", "https://claude.com/claude-code")),
        };
        s.message = format!("{label} isn't installed. {how}");
        s.next = Some(Next::Install);
        s.action = action;
        return;
    };
    s.detail = exe.display().to_string();
    let version = match probe(&exe, &["--version"]).await {
        Some((true, v)) => v.split_whitespace().find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit())).map(str::to_string),
        _ => {
            s.message = format!("{label} at {} doesn't start. Reinstall or update it.", s.detail);
            s.next = Some(Next::Install);
            return;
        }
    };
    let name = match &version {
        Some(v) => format!("{label} {v}"),
        None => label.to_string(),
    };
    let signed_in = match kind {
        ProviderKind::ClaudeCode => probe(&exe, &["auth", "status"]).await.and_then(|(_, out)| serde_json::from_str::<Value>(&out).ok().and_then(|v| v["loggedIn"].as_bool())),
        _ => probe(&exe, &["login", "status"]).await.map(|(ok, _)| ok),
    };
    if signed_in == Some(false) {
        let how = match kind {
            ProviderKind::Codex => "Run `codex login` in a terminal, then check again.",
            _ => "Run `claude auth login` in a terminal, then check again.",
        };
        s.message = format!("{name} is installed but signed out. {how}");
        s.next = Some(Next::SignIn);
        s.action = Action::run(
            "Copy the sign-in command",
            match kind {
                ProviderKind::Codex => "codex login",
                _ => "claude auth login",
            },
        );
        return;
    }
    if mcp_executable().is_none() {
        s.message = format!("{name} is installed, but folio-mcp, which connects it to folio, wasn't found. Reinstall folio.");
        s.next = Some(Next::Restart);
        return;
    }
    if session.bridge_port().is_none() {
        s.message = format!("{name} is installed, but folio's live bridge isn't running. Restart folio.");
        s.next = Some(Next::Restart);
        return;
    }
    let state = if signed_in == Some(true) { "installed and signed in" } else { "installed" };
    let account = match kind {
        ProviderKind::Codex => "ChatGPT/OpenAI",
        _ => "Claude",
    };
    s.ready = true;
    s.message = format!("{name} is {state}. It uses your own {account} account.");
}
