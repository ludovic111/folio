//! What can run the agent, as data: for each provider its group, label, wire protocol and
//! quirks, default model and address, where its key lives and where to get one, and a short
//! list of models for when its own list can't be fetched.
//!
//! The ids are `folio_control::settings::AGENT_PROVIDERS`; [`ProviderKind`] has one variant per
//! id. Keys live in the keychain under the ids `app.setAgentKey` takes (`anthropic`, `openai`,
//! `openrouter`, `gemini`, `mistral`).

use serde::Serialize;

use crate::ProviderKind;

/// How a provider is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    /// A coding CLI installed on this computer, with `folio-mcp --live` attached.
    Cli,
    /// A model API, with a key.
    Api,
    /// A model server on this computer or the local network.
    Local,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::Cli, Group::Api, Group::Local];

    pub fn label(self) -> &'static str {
        match self {
            Group::Cli => "On this computer",
            Group::Api => "Model APIs",
            Group::Local => "Local servers",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Group::Cli => "cli",
            Group::Api => "api",
            Group::Local => "local",
        }
    }
}

/// The request format a provider speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wire {
    Cli,
    Anthropic,
    /// Chat Completions, with the provider's quirks.
    Chat(Quirks),
    Gemini,
    Ollama,
}

/// Where Chat Completions servers differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quirks {
    /// The parameter that caps the reply, and the cap, when the default is too short or the
    /// server wants it named one way.
    pub max_tokens: Option<(&'static str, u32)>,
    /// Ask for token counts in the stream (`stream_options.include_usage`).
    pub usage: bool,
    /// At most this many tools in one request.
    pub tool_limit: Option<usize>,
    /// Only a short list of tools (small local models).
    pub compact: bool,
    /// Tool call ids must be 9 letters and digits (Mistral).
    pub short_ids: bool,
}

impl Quirks {
    pub const STANDARD: Quirks = Quirks { max_tokens: None, usage: true, tool_limit: None, compact: false, short_ids: false };
}

/// Where a provider's API key comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeySpec {
    /// Keychain id (`app.setAgentKey provider=…`).
    pub id: &'static str,
    /// Environment variables read when nothing is saved, in order.
    pub env: &'static [&'static str],
    /// A key is needed (else optional: a local server may ask for one).
    pub required: bool,
    /// Where people make one.
    pub url: Option<&'static str>,
    /// What one looks like, for the field's placeholder.
    pub hint: &'static str,
}

/// Everything static about a provider.
#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub kind: ProviderKind,
    pub id: &'static str,
    pub label: &'static str,
    pub group: Group,
    /// One plain line on what it is.
    pub tagline: &'static str,
    pub wire: Wire,
    /// Model used when none is chosen (empty: the CLI's or server's own).
    pub default_model: &'static str,
    /// Empty for the CLIs and custom servers (the person gives it).
    pub default_base_url: &'static str,
    /// The address is the person's to give (a custom server).
    pub needs_base_url: bool,
    /// What goes in the address field.
    pub base_url_hint: &'static str,
    pub key: Option<KeySpec>,
    /// Models shown when the list can't be fetched, the default first.
    pub models: &'static [&'static str],
    pub website: &'static str,
    /// The logo the window shows (`ui::logos`).
    pub logo: &'static str,
}

const fn key(id: &'static str, env: &'static [&'static str], url: &'static str, hint: &'static str) -> Option<KeySpec> {
    Some(KeySpec { id, env, required: true, url: Some(url), hint })
}

pub const ALL: &[Info] = &[
    // ---- on this computer ----
    Info {
        kind: ProviderKind::ClaudeCode,
        id: "claude-code",
        label: "Claude Code",
        group: Group::Cli,
        tagline: "Your Claude subscription, through the Claude Code app.",
        wire: Wire::Cli,
        default_model: "",
        default_base_url: "",
        needs_base_url: false,
        base_url_hint: "",
        key: None,
        models: &["opus", "sonnet", "haiku"],
        website: "https://claude.com/claude-code",
        logo: "claude",
    },
    Info {
        kind: ProviderKind::Codex,
        id: "codex",
        label: "Codex",
        group: Group::Cli,
        tagline: "Your ChatGPT plan, through OpenAI's Codex CLI.",
        wire: Wire::Cli,
        default_model: "",
        default_base_url: "",
        needs_base_url: false,
        base_url_hint: "",
        key: None,
        models: &[],
        website: "https://developers.openai.com/codex/cli",
        logo: "openai",
    },
    // ---- model APIs ----
    Info {
        kind: ProviderKind::Anthropic,
        id: "anthropic",
        label: "Anthropic API",
        group: Group::Api,
        tagline: "Claude models, billed per use by Anthropic.",
        wire: Wire::Anthropic,
        default_model: "claude-sonnet-5-5",
        default_base_url: "https://api.anthropic.com",
        needs_base_url: false,
        base_url_hint: "",
        key: key("anthropic", &["ANTHROPIC_API_KEY"], "https://platform.claude.com/settings/keys", "sk-ant-…"),
        models: &["claude-sonnet-5-5", "claude-opus-5-5", "claude-haiku-4-5"],
        website: "https://www.anthropic.com/api",
        logo: "claude",
    },
    Info {
        kind: ProviderKind::OpenAi,
        id: "openai",
        label: "OpenAI API",
        group: Group::Api,
        tagline: "GPT models, billed per use by OpenAI.",
        // OpenAI takes at most 128 functions in one request.
        wire: Wire::Chat(Quirks { tool_limit: Some(128), ..Quirks::STANDARD }),
        default_model: "gpt-5",
        default_base_url: "https://api.openai.com/v1",
        needs_base_url: false,
        base_url_hint: "",
        key: key("openai", &["OPENAI_API_KEY"], "https://platform.openai.com/api-keys", "sk-…"),
        models: &["gpt-5"],
        website: "https://platform.openai.com",
        logo: "openai",
    },
    Info {
        kind: ProviderKind::OpenRouter,
        id: "openrouter",
        label: "OpenRouter",
        group: Group::Api,
        tagline: "Hundreds of models from every lab with one key.",
        wire: Wire::Chat(Quirks::STANDARD),
        default_model: "anthropic/claude-sonnet-5.5",
        default_base_url: "https://openrouter.ai/api/v1",
        needs_base_url: false,
        base_url_hint: "",
        key: key("openrouter", &["OPENROUTER_API_KEY"], "https://openrouter.ai/settings/keys", "sk-or-…"),
        models: &["anthropic/claude-sonnet-5.5"],
        website: "https://openrouter.ai",
        logo: "openrouter",
    },
    Info {
        kind: ProviderKind::Gemini,
        id: "gemini",
        label: "Google Gemini API",
        group: Group::Api,
        tagline: "Gemini models with a Google AI Studio key; there is a free tier.",
        wire: Wire::Gemini,
        default_model: "gemini-3.1-pro-preview",
        default_base_url: "https://generativelanguage.googleapis.com/v1beta",
        needs_base_url: false,
        base_url_hint: "",
        key: key("gemini", &["GEMINI_API_KEY", "GOOGLE_API_KEY"], "https://aistudio.google.com/apikey", "AIza…"),
        models: &["gemini-3.1-pro-preview"],
        website: "https://ai.google.dev",
        logo: "gemini",
    },
    Info {
        kind: ProviderKind::Mistral,
        id: "mistral",
        label: "Mistral",
        group: Group::Api,
        tagline: "Mistral's own models, from Europe.",
        wire: Wire::Chat(Quirks { short_ids: true, ..Quirks::STANDARD }),
        default_model: "mistral-large-latest",
        default_base_url: "https://api.mistral.ai/v1",
        needs_base_url: false,
        base_url_hint: "",
        key: key("mistral", &["MISTRAL_API_KEY"], "https://console.mistral.ai/api-keys", ""),
        models: &["mistral-large-latest"],
        website: "https://mistral.ai",
        logo: "mistral",
    },
    // ---- local servers ----
    Info {
        kind: ProviderKind::Ollama,
        id: "ollama",
        label: "Ollama",
        group: Group::Local,
        tagline: "Models on this computer. Nothing leaves it.",
        wire: Wire::Ollama,
        default_model: "",
        default_base_url: "http://127.0.0.1:11434",
        needs_base_url: false,
        base_url_hint: "",
        key: None,
        models: &[],
        website: "https://ollama.com",
        logo: "ollama",
    },
    Info {
        kind: ProviderKind::LmStudio,
        id: "lmstudio",
        label: "LM Studio",
        group: Group::Local,
        tagline: "Models on this computer, from the LM Studio app.",
        wire: Wire::Chat(Quirks { usage: false, compact: true, ..Quirks::STANDARD }),
        default_model: "",
        default_base_url: "http://127.0.0.1:1234/v1",
        needs_base_url: false,
        base_url_hint: "",
        key: Some(KeySpec { id: "lmstudio", env: &["LM_API_TOKEN"], required: false, url: None, hint: "Only if the server asks for one" }),
        models: &[],
        website: "https://lmstudio.ai",
        logo: "lmstudio",
    },
    Info {
        kind: ProviderKind::OpenAiCompatible,
        id: "openai-compatible",
        label: "OpenAI-compatible server",
        group: Group::Local,
        tagline: "Any server that speaks OpenAI's Chat Completions: vLLM, llama.cpp, LiteLLM, a proxy…",
        wire: Wire::Chat(Quirks { usage: false, tool_limit: Some(128), ..Quirks::STANDARD }),
        default_model: "",
        default_base_url: "",
        needs_base_url: true,
        base_url_hint: "The server's address, e.g. http://127.0.0.1:8000/v1",
        key: Some(KeySpec { id: "openai-compatible", env: &[], required: false, url: None, hint: "Only if the server asks for one" }),
        models: &[],
        website: "",
        logo: "",
    },
];

/// The static facts about `kind`.
pub fn info(kind: ProviderKind) -> &'static Info {
    ALL.iter().find(|i| i.kind == kind).expect("every provider is in providers::ALL")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_matches_the_settings_ids() {
        let ids: Vec<&str> = ALL.iter().map(|i| i.id).collect();
        assert_eq!(ids, folio_control::settings::AGENT_PROVIDERS);
        assert_eq!(ALL[0].kind, ProviderKind::ClaudeCode, "the person's own Claude Code comes first");
        for i in ALL {
            assert_eq!(i.kind.id(), i.id);
            assert_eq!(ProviderKind::parse(i.id), Some(i.kind));
            assert_eq!(i.group == Group::Cli, i.wire == Wire::Cli, "{}", i.id);
            if let Some(k) = i.key.filter(|k| k.required) {
                assert!(k.url.is_some(), "{}: a key is needed, so say where to get one", i.id);
            }
            if !i.default_model.is_empty() && !i.models.is_empty() {
                assert_eq!(i.models[0], i.default_model, "{}: the default model comes first", i.id);
            }
        }
    }
}
