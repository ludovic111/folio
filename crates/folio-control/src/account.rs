//! lsuite AI: the one account of the suite (see lsuite's AI.md).
//!
//! Signing in from any lsuite app signs in every app on the computer: the account lives in
//! `~/.lsuite/account.json` (0600, written atomically; `LSUITE_HOME` replaces `~/.lsuite`), read
//! again whenever it is needed since another app may change it. The token is a secret: never
//! logged, never put in a document, masked in answers.
//!
//! Signing in works like native apps' OAuth: folio listens on a random loopback port, opens
//! `<server>/account/connect?app=folio&port=…&state=…` in the browser, and the page sends the
//! browser back to `http://127.0.0.1:<port>/callback?code=…&state=…`; the code is exchanged for a
//! token with `POST <server>/api/account/token`. Headless: a key from the account page (`lsk_…`).

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::session::CmdResult;

pub const DEFAULT_SERVER: &str = "https://lsuite.xyz";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountFile {
    pub format: u32,
    pub server: String,
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub plan: String,
    pub token: String,
    pub signed_in_at: chrono::DateTime<chrono::Utc>,
}

/// `$LSUITE_HOME`, else `~/.lsuite`.
pub fn lsuite_home() -> PathBuf {
    if let Some(p) = std::env::var_os("LSUITE_HOME").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    dirs::home_dir().unwrap_or_else(std::env::temp_dir).join(".lsuite")
}

pub fn path() -> PathBuf {
    lsuite_home().join("account.json")
}

/// The account on this computer, if signed in.
pub fn read() -> Option<AccountFile> {
    let bytes = std::fs::read(path()).ok()?;
    serde_json::from_slice::<AccountFile>(&bytes).ok().filter(|a| a.format == 1 && !a.token.is_empty())
}

pub fn write(a: &AccountFile) -> CmdResult<()> {
    let p = path();
    let json = serde_json::to_vec_pretty(a).map_err(|e| e.to_string())?;
    write_private(&p, &json).map_err(|e| format!("Couldn't write {}: {e}", p.display()))
}

fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        f.write_all(bytes)?;
    }
    #[cfg(not(unix))]
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

/// The server: `LSUITE_ACCOUNT_SERVER`, else the signed-in account's, else lsuite.xyz.
pub fn server() -> String {
    if let Ok(s) = std::env::var("LSUITE_ACCOUNT_SERVER")
        && !s.trim().is_empty()
    {
        return s.trim().trim_end_matches('/').to_string();
    }
    read().map(|a| a.server).filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

/// The Anthropic-compatible endpoint the agent talks to (`<server>/api/ai`).
pub fn api_base() -> String {
    format!("{}/api/ai", server())
}

/// A token shown in answers: its start and end only.
pub fn mask(token: &str) -> String {
    if token.len() <= 10 { "••••".into() } else { format!("{}…{}", &token[..6], &token[token.len() - 4..]) }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().user_agent(concat!("folio/", env!("CARGO_PKG_VERSION"))).timeout(Duration::from_secs(15)).build().unwrap_or_default()
}

/// One line from an error in Anthropic's shape (`{type: "error", error: {type, message}}`).
pub fn error_line(status: u16, body: &Value) -> String {
    let msg = body["error"]["message"].as_str().map(str::to_string).unwrap_or_else(|| format!("The lsuite server answered {status}."));
    match body["error"]["type"].as_str() {
        Some("allowance_exhausted") => format!("{msg} Manage plan: {}", body["error"]["manage_url"].as_str().unwrap_or("https://lsuite.xyz/account")),
        Some("plan_required") | Some("model_not_in_plan") => format!("{msg} Manage plan: {}", body["error"]["manage_url"].as_str().unwrap_or("https://lsuite.xyz/account")),
        _ => msg,
    }
}

/// `GET /api/account/me` with a token.
pub async fn me(server: &str, token: &str) -> CmdResult<Value> {
    let r = client().get(format!("{server}/api/account/me")).bearer_auth(token).send().await.map_err(|e| format!("Couldn't reach {server}: {e}"))?;
    let status = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    if status != 200 {
        return Err(error_line(status, &body));
    }
    Ok(body)
}

/// `GET /api/ai/plans`.
pub async fn plans() -> CmdResult<Value> {
    let server = server();
    let r = client().get(format!("{server}/api/ai/plans")).send().await.map_err(|e| format!("Couldn't reach {server}: {e}"))?;
    let status = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    if status != 200 {
        return Err(error_line(status, &body));
    }
    Ok(body)
}

/// What `account.status` answers.
pub async fn status(refresh: bool) -> CmdResult<Value> {
    let Some(acc) = read() else {
        return Ok(json!({ "signedIn": false, "server": server(), "pitch": "No setup. Sign in and your agent works.", "manageUrl": format!("{}/account", server()) }));
    };
    let mut v = json!({
        "signedIn": true,
        "server": acc.server,
        "email": acc.email,
        "name": acc.name,
        "plan": acc.plan,
        "token": mask(&acc.token),
        "manageUrl": format!("{}/account", acc.server),
    });
    // Always asked again: the plan and the allowance change on the server (`refresh` is kept
    // for clients that want to say so).
    let _ = refresh;
    {
        match me(&acc.server, &acc.token).await {
            Ok(m) => {
                if m["plan"].as_str().is_some_and(|p| p != acc.plan) {
                    let mut a2 = acc.clone();
                    a2.plan = m["plan"].as_str().unwrap_or("").to_string();
                    let _ = write(&a2);
                }
                v["plan"] = m["plan"].clone();
                v["planName"] = m["planName"].clone();
                v["status"] = m["status"].clone();
                v["usage"] = m["usage"].clone();
                v["models"] = m["models"].clone();
                v["defaultModel"] = m["defaultModel"].clone();
                v["demo"] = m["demo"].clone();
                if let Some(u) = m["manageUrl"].as_str() {
                    v["manageUrl"] = json!(u);
                }
                v["summary"] = json!(summary(&m));
            }
            Err(e) => {
                v["offline"] = json!(true);
                v["error"] = json!(e);
            }
        }
    }
    Ok(v)
}

/// "Pro · 38 % used · resets 1 Nov".
pub fn summary(me: &Value) -> String {
    let plan = me["planName"].as_str().or(me["plan"].as_str()).unwrap_or("Free");
    let pct = me["usage"]["percent"].as_f64().unwrap_or(0.0);
    let resets = me["usage"]["resetsAt"].as_str().and_then(|r| chrono::DateTime::parse_from_rfc3339(r).ok()).map(|d| d.format("%-d %b").to_string());
    match resets {
        Some(r) if me["usage"]["limit"].as_f64().unwrap_or(0.0) > 0.0 => format!("{plan} · {pct:.0} % used · resets {r}"),
        _ => plan.to_string(),
    }
}

/// Signs in with a key from the account page.
pub async fn sign_in_key(key: &str) -> CmdResult<Value> {
    let server = server();
    let key = key.trim();
    let m = me(&server, key).await?;
    let acc = AccountFile {
        format: 1,
        server: server.clone(),
        email: m["email"].as_str().unwrap_or("").to_string(),
        name: m["name"].as_str().unwrap_or("").to_string(),
        plan: m["plan"].as_str().unwrap_or("").to_string(),
        token: key.to_string(),
        signed_in_at: chrono::Utc::now(),
    };
    write(&acc)?;
    Ok(json!({ "signedIn": true, "email": acc.email, "plan": acc.plan, "summary": summary(&m) }))
}

/// A browser sign-in in progress: where the browser was sent, and the task finishing it.
pub struct BrowserSignIn {
    pub url: String,
    pub done: tokio::task::JoinHandle<CmdResult<Value>>,
}

/// Starts the loopback sign-in: listens, opens the browser, and finishes when the page sends
/// the browser back (or after five minutes).
pub async fn sign_in_browser(open_browser: bool) -> CmdResult<BrowserSignIn> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let server = server();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| format!("Couldn't listen for the sign-in: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let state = crate::bridge::token();
    let url = format!("{server}/account/connect?app=folio&port={port}&state={state}");
    if open_browser {
        open_url(&url);
    }
    let done = tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
        loop {
            let accepted = tokio::time::timeout_at(deadline, listener.accept()).await;
            let Ok(Ok((mut stream, peer))) = accepted else { return Err("The sign-in wasn't finished in the browser within 5 minutes.".to_string()) };
            if !peer.ip().is_loopback() {
                continue;
            }
            let mut buf = vec![0u8; 8192];
            let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await.ok().and_then(Result::ok).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let target = req.split_whitespace().nth(1).unwrap_or("").to_string();
            if !target.starts_with("/callback") {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                continue;
            }
            let q = url::Url::parse(&format!("http://127.0.0.1{target}")).ok();
            let param = |k: &str| q.as_ref().and_then(|u| u.query_pairs().find(|(a, _)| a == k).map(|(_, v)| v.into_owned()));
            let (code, st) = (param("code"), param("state"));
            let page = |ok: bool, msg: &str| {
                let body = format!(
                    "<!doctype html><meta charset=utf-8><title>folio</title><body style=\"font-family:system-ui;background:#050505;color:#f2f2f2;display:grid;place-items:center;height:100vh;margin:0\"><div><h1 style=\"font-weight:600\">{}</h1><p>{msg}</p></div>",
                    if ok { "folio is connected" } else { "Sign-in didn't finish" }
                );
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
            };
            if st.as_deref() != Some(state.as_str()) || code.is_none() {
                let _ = stream.write_all(page(false, "This page didn't come from folio's sign-in. Start again from folio.").as_bytes()).await;
                continue;
            }
            let result = exchange(&server, &code.unwrap()).await;
            let msg = match &result {
                Ok(_) => "You can close this tab and go back to folio.".to_string(),
                Err(e) => e.clone(),
            };
            let _ = stream.write_all(page(result.is_ok(), &msg).as_bytes()).await;
            return result;
        }
    });
    Ok(BrowserSignIn { url, done })
}

/// `POST /api/account/token {code}` → the account file.
async fn exchange(server: &str, code: &str) -> CmdResult<Value> {
    let r = client().post(format!("{server}/api/account/token")).json(&json!({ "code": code })).send().await.map_err(|e| format!("Couldn't reach {server}: {e}"))?;
    let status = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    if status != 200 {
        return Err(error_line(status, &body));
    }
    let token = body["token"].as_str().ok_or("The server didn't send a token.")?.to_string();
    let a = &body["account"];
    let acc = AccountFile {
        format: 1,
        server: server.to_string(),
        email: a["email"].as_str().unwrap_or("").to_string(),
        name: a["name"].as_str().unwrap_or("").to_string(),
        plan: a["plan"].as_str().unwrap_or("").to_string(),
        token,
        signed_in_at: chrono::Utc::now(),
    };
    write(&acc)?;
    Ok(json!({ "signedIn": true, "email": acc.email, "plan": acc.plan, "summary": summary(a) }))
}

/// Signs out here and on the server (the token is revoked).
pub async fn sign_out() -> CmdResult<Value> {
    let Some(acc) = read() else { return Ok(json!({ "signedIn": false })) };
    let _ = client().post(format!("{}/api/account/signout", acc.server)).bearer_auth(&acc.token).send().await;
    let _ = std::fs::remove_file(path());
    Ok(json!({ "signedIn": false, "was": acc.email }))
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
