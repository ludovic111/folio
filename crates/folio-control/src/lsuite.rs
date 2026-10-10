//! lsuite, the suite folio belongs to: its folder on this computer and its server.
//!
//! `~/.lsuite` (`LSUITE_HOME` replaces it) holds what every lsuite app shares: plugins and the
//! discovery files. The server (`LSUITE_SERVER`, else lsuite.xyz) is where the builds come from
//! (see [`crate::update`]). No account is needed for anything.

use std::path::PathBuf;

pub const DEFAULT_SERVER: &str = "https://lsuite.xyz";

/// `$LSUITE_HOME`, else `~/.lsuite`.
pub fn lsuite_home() -> PathBuf {
    if let Some(p) = std::env::var_os("LSUITE_HOME").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    dirs::home_dir().unwrap_or_else(std::env::temp_dir).join(".lsuite")
}

/// The lsuite server: `LSUITE_SERVER`, else lsuite.xyz.
pub fn server() -> String {
    std::env::var("LSUITE_SERVER").ok().map(|s| s.trim().trim_end_matches('/').to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

/// Opens a URL in the person's browser.
pub fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").args(["/c", "start", "", url]).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        tracing::warn!("couldn't open the browser: {e}");
    }
}
