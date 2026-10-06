//! App settings, saved as `settings.json` in the config folder.
//!
//! `agent.permissions` is the one place agent and MCP requests are checked against (see
//! [`crate::registry::Perm`]); off means off for both.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub agent: AgentSettings,
    pub appearance: Appearance,
    pub editing: Editing,
    pub updates: UpdateSettings,
    pub onboarding: OnboardingSettings,
    pub plugins: PluginSettings,
}

/// What can run the built-in agent (`settings.agent.provider`): lsuite AI first, then the
/// person's own coding CLIs, model APIs and local servers.
pub const AGENT_PROVIDERS: &[&str] = &["lsuite", "claude-code", "codex", "anthropic", "openai", "openrouter", "gemini", "mistral", "ollama", "lmstudio", "openai-compatible"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentSettings {
    /// The Agent panel is offered (off hides it; MCP and the CLI still work).
    pub enabled: bool,
    pub permissions: Permissions,
    /// Which model runs the built-in agent, one of [`AGENT_PROVIDERS`].
    pub provider: String,
    /// Model id for the API and local providers (empty: the provider's default).
    pub model: String,
    /// Base URL for local and OpenAI-compatible servers.
    pub base_url: String,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self { enabled: true, permissions: Permissions::default(), provider: "lsuite".into(), model: String::new(), base_url: String::new() }
    }
}

/// What an agent (the built-in one, MCP clients, `folio-cli --agent`) may do besides editing the
/// open file, which is always allowed and always undoable.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Permissions {
    /// Master switch: off refuses every agent and MCP request.
    pub enabled: bool,
    /// Open, import, export and write files; insert pictures from disk.
    pub files: bool,
    /// Change settings other than these permissions and keys.
    pub settings: bool,
    /// Build, install and remove plugins (runs the Rust compiler, loads native code).
    pub plugins: bool,
    /// Quit the app, drive other lsuite apps.
    pub app_control: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Self { enabled: true, files: true, settings: false, plugins: false, app_control: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Appearance {
    /// `system`, `dark` or `light`.
    pub mode: String,
    /// Translucent chrome over the grain; off gives opaque surfaces.
    pub transparency: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { mode: "system".into(), transparency: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Editing {
    /// The name on comments and tracked changes.
    pub author: String,
    /// Page size for new documents: `a4` or `letter`.
    pub paper: String,
    /// Check spelling (not yet: kept for the next release).
    pub spelling: bool,
}

impl Default for Editing {
    fn default() -> Self {
        let letter = std::env::var("LANG").is_ok_and(|l| l.contains("_US") || l.contains("_CA"));
        Self { author: String::new(), paper: if letter { "letter".into() } else { "a4".into() }, spelling: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateSettings {
    /// Look for a newer release on GitHub when the app starts. `FOLIO_NO_UPDATE=1` also turns it off.
    pub check_on_start: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { check_on_start: true }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct OnboardingSettings {
    /// The folio version the first-run setup was finished in; empty: never (it shows at start).
    pub completed: String,
    /// The suite the person said they came from (`office`, `google`, `apple`, `libreoffice`), or empty.
    pub coming_from: String,
}

impl OnboardingSettings {
    pub fn is_done(&self) -> bool {
        !self.completed.trim().is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct PluginSettings {
    /// Plugin ids switched off (stock and installed alike).
    pub disabled: Vec<String>,
}

impl Settings {
    /// Reads `settings.json`. A file that can't be read as settings is kept as
    /// `settings.json.bad` (so the next save doesn't lose it) and the defaults are used.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("settings.json");
        let Ok(bytes) = std::fs::read(&path) else { return Self::default() };
        match serde_json::from_slice(&bytes) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("{} isn't valid ({e}); kept as settings.json.bad, using the defaults", path.display());
                let _ = std::fs::rename(&path, dir.join("settings.json.bad"));
                Self::default()
            }
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let tmp = dir.join("settings.json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, dir.join("settings.json"))
    }

    pub fn update_check_enabled(&self) -> bool {
        self.updates.check_on_start && !std::env::var("FOLIO_NO_UPDATE").is_ok_and(|v| !v.is_empty() && v != "0")
    }

    /// The author name for comments and tracked changes.
    pub fn author(&self) -> String {
        if !self.editing.author.trim().is_empty() {
            return self.editing.author.trim().to_string();
        }
        std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "Me".into())
    }

    /// Reads a setting by dotted key (`appearance.mode`).
    pub fn get(&self, key: &str) -> Option<Value> {
        let mut v = serde_json::to_value(self).ok()?;
        for part in key.split('.').filter(|p| !p.is_empty()) {
            v = v.get(part)?.clone();
        }
        Some(v)
    }

    /// Sets a setting by dotted key, keeping the value's type.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        let mut root = serde_json::to_value(&*self).map_err(|e| e.to_string())?;
        let parts: Vec<&str> = key.split('.').filter(|p| !p.is_empty()).collect();
        let (last, path) = parts.split_last().ok_or("empty setting key")?;
        let mut node = &mut root;
        for p in path {
            node = node.get_mut(*p).ok_or_else(|| unknown(key))?;
        }
        let slot = node.get_mut(*last).ok_or_else(|| unknown(key))?;
        let same_type = matches!((&*slot, &value), (Value::Bool(_), Value::Bool(_)) | (Value::String(_), Value::String(_)) | (Value::Number(_), Value::Number(_)))
            || matches!((&*slot, &value), (Value::Array(_), Value::Array(items)) if items.iter().all(Value::is_string));
        if !same_type {
            return Err(format!("`{key}` expects {}, got {value}", type_name(slot)));
        }
        if let Some(allowed) = choices(key)
            && !value.as_str().is_some_and(|v| allowed.contains(&v))
        {
            return Err(format!("`{key}` is one of {}, not {value}.", allowed.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(", ")));
        }
        *slot = value;
        *self = serde_json::from_value(root).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// The values a text setting takes, for those with a fixed set.
pub fn choices(key: &str) -> Option<&'static [&'static str]> {
    Some(match key {
        "appearance.mode" => &["system", "dark", "light"],
        "agent.provider" => AGENT_PROVIDERS,
        "editing.paper" => &["a4", "letter"],
        "onboarding.comingFrom" => &["", "office", "google", "apple", "libreoffice", "none"],
        _ => return None,
    })
}

fn unknown(key: &str) -> String {
    format!("Unknown setting `{key}`. app.settings lists them.")
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list of strings",
        _ => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_checks_types_and_choices() {
        let mut s = Settings::default();
        s.set("appearance.mode", Value::String("dark".into())).unwrap();
        assert_eq!(s.appearance.mode, "dark");
        assert!(s.set("appearance.mode", Value::String("blue".into())).is_err());
        assert!(s.set("agent.permissions.files", Value::String("yes".into())).is_err());
        assert!(s.set("nope.key", Value::Bool(true)).is_err());
    }
}
