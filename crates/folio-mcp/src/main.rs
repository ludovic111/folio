//! `folio-mcp`: folio as a Model Context Protocol server over stdio (newline-delimited
//! JSON-RPC 2.0). Every registry command is a tool (`sheet.set` becomes `sheet_set`), with
//! its description and JSON schema taken from the registry, so the tools can't drift from what
//! the window, the CLI and the built-in agent accept.
//!
//! In live mode each call runs in the open folio window through its loopback bridge, so an
//! agent and the person edit the same file with one undo history. With `--file` the server
//! hosts a session on a file and saves after every change. MCP requests are always held
//! to Settings › Agent › Permissions. Only protocol goes to stdout; logs go to stderr.
//!
//! Requests are served concurrently: each one is a task, so `ping` is answered while an export
//! runs, and `notifications/cancelled` aborts the request it names. One writer
//! thread owns stdout, one JSON line at a time.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use folio_cli::Backend;
use folio_control::{Perm, Source, registry};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

mod prompts;

/// Protocol revisions we speak, oldest first; an unknown request gets the newest.
const PROTOCOLS: [&str; 4] = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];
const MAX_LINE: usize = 64 * 1024 * 1024;

const USAGE: &str = "folio-mcp — Model Context Protocol server for folio (stdio)

USAGE
  folio-mcp                  drive the running folio app; if it isn't running, host a session in
                             this process (file_new or file_open first)
  folio-mcp --live           drive the running app only (calls fail with a hint while it is closed)
  folio-mcp --file <path>    host that file in this process and save after every change;
                             file_new makes a .folio file that doesn't exist yet
  folio-mcp --headless       host a session in this process with no file open

Register it with an MCP client, for example Claude Code:
  claude mcp add folio -- /path/to/folio-mcp --live
`folio-cli mcp-config` prints this line and the others with this computer's paths.";

#[tokio::main]
async fn main() {
    let mut file: Option<PathBuf> = None;
    let (mut live, mut headless) = (false, false);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            "--version" | "-V" => {
                println!("folio-mcp {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--file" | "-f" => match args.next().filter(|p| !p.starts_with("--")) {
                Some(p) => file = Some(PathBuf::from(p)),
                None => exit_usage("--file needs a path"),
            },
            "--live" => live = true,
            "--headless" => headless = true,
            _ => exit_usage(&format!("Unknown option `{arg}`")),
        }
    }
    if usize::from(file.is_some()) + usize::from(live) + usize::from(headless) > 1 {
        exit_usage("--file, --live and --headless are exclusive");
    }
    let backend = match (file, live, headless) {
        (Some(path), _, _) => Backend::file(&path, Source::Mcp).await,
        (None, true, _) => {
            if let Err(e) = Backend::live("mcp").await {
                eprintln!("folio-mcp: {e}\nfolio-mcp: waiting for the app; each call tries again");
            }
            Ok(Backend::live_lazy("mcp"))
        }
        (None, false, true) => Backend::headless(Source::Mcp).await,
        (None, false, false) => match Backend::live("mcp").await {
            Ok(b) => Ok(b),
            Err(e) => {
                eprintln!("folio-mcp: {e}\nfolio-mcp: hosting a session in this process instead");
                Backend::headless(Source::Mcp).await
            }
        },
    };
    let backend = match backend {
        Ok(b) => b,
        Err(e) => {
            eprintln!("folio-mcp: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("folio-mcp {}: {} mode{}", env!("CARGO_PKG_VERSION"), backend.mode(), backend.path().map(|p| format!(" on {}", p.display())).unwrap_or_default());

    let (out, writer) = protocol_out();
    let server = Arc::new(Server::new(backend));
    // Requests in flight by id (as JSON text), to cancel them.
    let running: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>> = Arc::default();
    let mut stdin = tokio::io::BufReader::with_capacity(1 << 16, tokio::io::stdin());
    loop {
        let line = match read_line(&mut stdin, MAX_LINE).await {
            Ok(Line::Text(l)) => l,
            Ok(Line::TooLong) => {
                eprintln!("folio-mcp: skipped a request over 64 MiB");
                out.send(error(Value::Null, -32600, "The request exceeds 64 MiB"));
                continue;
            }
            Ok(Line::Eof) => break,
            Err(e) => {
                eprintln!("folio-mcp: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let frame = match server.frame(&line) {
            Ok(f) => f,
            Err(reply) => {
                out.send(reply);
                continue;
            }
        };
        let Some(Frame { id, method, params }) = frame else { continue };
        let Some(id) = id else {
            // Notifications get no answer; a cancelled request gets none either.
            if method == "notifications/cancelled"
                && let Some(task) = params.get("requestId").and_then(|r| running.lock().unwrap_or_else(|e| e.into_inner()).remove(&r.to_string()))
            {
                task.abort();
            }
            continue;
        };
        let key = id.to_string();
        let (server, out, done) = (server.clone(), out.clone(), running.clone());
        // Held while spawning, so a quick task can't remove its entry before it is there.
        let mut tasks = running.lock().unwrap_or_else(|e| e.into_inner());
        let task_key = key.clone();
        let task = tokio::spawn(async move {
            let reply = match server.dispatch(&method, &params).await {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err((code, message)) => error(id, code, &message),
            };
            done.lock().unwrap_or_else(|e| e.into_inner()).remove(&task_key);
            out.send(reply);
        });
        tasks.insert(key, task);
    }
    // The client is gone; let calls already running finish (a file-mode edit saves its file),
    // up to a few seconds.
    let left: Vec<_> = running.lock().unwrap_or_else(|e| e.into_inner()).drain().map(|(_, t)| t).collect();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        for t in left {
            let _ = t.await;
        }
    })
    .await;
    // Out with the answers already given, then stop (a call still running keeps a sender: bounded).
    drop(out);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), tokio::task::spawn_blocking(move || writer.join())).await;
}

/// One line of input, read without ever holding more than `max` bytes of it.
#[derive(Debug, PartialEq)]
enum Line {
    Text(String),
    /// Over `max`: skipped up to its end.
    TooLong,
    Eof,
}

async fn read_line(r: &mut (impl AsyncBufRead + Unpin), max: usize) -> std::io::Result<Line> {
    let mut buf: Vec<u8> = vec![];
    let mut over = false;
    loop {
        let chunk = r.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(match (over, buf.is_empty()) {
                (true, _) => Line::TooLong,
                (false, true) => Line::Eof,
                (false, false) => Line::Text(String::from_utf8_lossy(&buf).into_owned()),
            });
        }
        let newline = chunk.iter().position(|b| *b == b'\n');
        let take = newline.map_or(chunk.len(), |i| i + 1);
        if !over && buf.len() + take > max + 1 {
            over = true;
            buf = vec![];
        }
        if !over {
            buf.extend_from_slice(&chunk[..take]);
        }
        r.consume(take);
        if newline.is_some() {
            if over {
                return Ok(Line::TooLong);
            }
            while buf.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
                buf.pop();
            }
            return Ok(Line::Text(String::from_utf8_lossy(&buf).into_owned()));
        }
    }
}

/// The protocol's way out: a thread that writes one JSON line per message. When stdout is gone
/// the client is too, and the server stops.
#[derive(Clone)]
struct Out(std::sync::mpsc::Sender<Value>);

impl Out {
    fn send(&self, v: Value) {
        let _ = self.0.send(v);
    }
}

fn protocol_out() -> (Out, std::thread::JoinHandle<()>) {
    let mut stdout = protocol_stdout();
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    let writer = std::thread::spawn(move || {
        for v in rx {
            let written = serde_json::to_writer(&mut stdout, &v).map_err(std::io::Error::other).and_then(|()| stdout.write_all(b"\n")).and_then(|()| stdout.flush());
            if written.is_err() {
                std::process::exit(0);
            }
        }
    });
    (Out(tx), writer)
}

/// stdout for the protocol only. On Unix the real stdout is kept aside and fd 1 points at
/// stderr from here on, so a stray print (ours, a library's or a child process's, like ffmpeg)
/// can never corrupt the JSON-RPC stream.
#[cfg(unix)]
fn protocol_stdout() -> Box<dyn Write + Send> {
    use std::os::fd::FromRawFd;
    unsafe extern "C" {
        fn dup(fd: i32) -> i32;
        fn dup2(old: i32, new: i32) -> i32;
    }
    // SAFETY: plain descriptor calls on 0–2, which exist for the life of the process; the
    // duplicate is owned by the File alone.
    unsafe {
        let fd = dup(1);
        if fd >= 0 && dup2(2, 1) >= 0 {
            return Box::new(std::io::BufWriter::new(std::fs::File::from_raw_fd(fd)));
        }
    }
    Box::new(std::io::stdout())
}

#[cfg(not(unix))]
fn protocol_stdout() -> Box<dyn Write + Send> {
    Box::new(std::io::stdout())
}

fn exit_usage(message: &str) -> ! {
    eprintln!("{message}\n\n{USAGE}");
    std::process::exit(2);
}

struct Server {
    backend: Backend,
    /// The live context last given (its `seq` and text), so a tool result carries a fresh one
    /// only when something changed.
    context: Mutex<Option<(u64, String)>>,
    /// Set by an edit, cleared by `harness.look` or `harness.check`: while set, results remind
    /// the agent of the finish routine.
    unchecked: Mutex<bool>,
}

/// The finish routine's reminder, in results after an edit until the agent checks or looks.
const NOT_CHECKED: &str = "Not checked yet: before you say you are done, run harness_check and harness_look on what you changed (the finish routine), fix what they show, then report.";

/// A request (with an id) or a notification.
struct Frame {
    id: Option<Value>,
    method: String,
    params: Value,
}

impl Server {
    /// Reads one line: `Ok(None)` for something to ignore (a client's response), `Err` for the
    /// error answer to a bad frame.
    fn frame(&self, line: &str) -> Result<Option<Frame>, Value> {
        let frame: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Err(error(Value::Null, -32700, &format!("Parse error: {e}"))),
        };
        let Some(obj) = frame.as_object() else {
            return Err(error(Value::Null, -32600, "Batch requests are not supported"));
        };
        let id = obj.get("id").cloned();
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || obj.get("method").and_then(Value::as_str).is_none_or(str::is_empty)
            || id.as_ref().is_some_and(|id| !id.is_null() && !id.is_string() && !id.is_number())
        {
            // A response from the client (we send no requests) or a malformed frame.
            if obj.contains_key("result") || obj.contains_key("error") {
                return Ok(None);
            }
            return Err(error(id.unwrap_or(Value::Null), -32600, "Invalid JSON-RPC 2.0 request"));
        }
        let method = obj.get("method").and_then(Value::as_str).unwrap_or("").to_string();
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        Ok(Some(Frame { id, method, params }))
    }

    async fn dispatch(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
                let version = if PROTOCOLS.contains(&requested) { requested } else { PROTOCOLS[PROTOCOLS.len() - 1] };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {
                        "tools": { "listChanged": false },
                        "resources": { "subscribe": false, "listChanged": false },
                        "prompts": { "listChanged": false },
                    },
                    "serverInfo": { "name": "folio", "title": "folio: documents, sheets and slides", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": self.instructions(),
                }))
            }
            "ping" => Ok(json!({})),
            "logging/setLevel" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "tools/call needs `name`".to_string()))?;
                let spec = registry::commands()
                    .iter()
                    .find(|s| s.tool_name() == name || s.name == name)
                    .ok_or_else(|| (-32602, format!("Unknown tool `{name}`")))?;
                if for_builtin_agent() && spec.family() == "agent" {
                    return Err((-32602, format!("Unknown tool `{name}`: the built-in agent doesn't drive itself")));
                }
                let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
                let result = self.backend.call(spec.name, arguments).await;
                let pictures = match &result {
                    Ok(value) => {
                        let mut pictures = vec![];
                        for path in folio_control::vision::pictures_in(spec.name, value) {
                            pictures.push(match folio_control::vision::picture(&path).await {
                                Ok(p) => json!({ "type": "image", "data": p.data, "mimeType": p.media_type }),
                                Err(e) => json!({ "type": "text", "text": format!("(The picture couldn't be read: {e})") }),
                            });
                        }
                        pictures
                    }
                    Err(_) => vec![],
                };
                let notes = self.notes(spec, result.is_ok()).await;
                Ok(tool_reply(result, notes, pictures))
            }
            "resources/list" => {
                let mut list: Vec<Value> = RESOURCES.iter().map(|(uri, name, description, _)| json!({
                    "uri": uri, "name": name, "description": description, "mimeType": "application/json",
                })).collect();
                list.push(json!({ "uri": "folio://harness/brief", "name": "Brief", "description": "The expert brief: how to do office work well in folio, the finish routine and the skills.", "mimeType": "text/markdown" }));
                list.push(json!({ "uri": "folio://harness/context", "name": "Live context", "description": "What the file holds now, what the window shows, open problems.", "mimeType": "text/plain" }));
                for k in folio_control::harness::skills() {
                    list.push(json!({ "uri": format!("folio://skills/{}", k.name), "name": format!("Skill: {}", k.title), "description": k.when, "mimeType": "text/markdown" }));
                }
                Ok(json!({ "resources": list }))
            }
            "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).ok_or((-32602, "resources/read needs `uri`".to_string()))?;
                let markdown = |text: String| json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": text }] });
                if uri == "folio://harness/brief" {
                    return Ok(markdown(folio_control::harness::brief().to_string()));
                }
                if uri == "folio://harness/context" {
                    let v = self.backend.call("harness.context", json!({})).await.map_err(|e| (-32000, e))?;
                    return Ok(json!({ "contents": [{ "uri": uri, "mimeType": "text/plain", "text": v["text"].as_str().unwrap_or("") }] }));
                }
                if let Some(name) = uri.strip_prefix("folio://skills/") {
                    let skill = folio_control::harness::skill(name).map_err(|e| (-32002, e))?;
                    return Ok(markdown(skill.markdown.to_string()));
                }
                let command = RESOURCES.iter().find(|(u, ..)| *u == uri).map(|(.., c)| *c).ok_or((-32002, format!("Unknown resource `{uri}`")))?;
                let value = self.backend.call(command, json!({})).await.map_err(|e| (-32000, e))?;
                Ok(json!({ "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&value).unwrap_or_default(),
                }] }))
            }
            "prompts/list" => {
                let mut list: Vec<Value> = prompts::PROMPTS.iter().map(|p| json!({
                    "name": p.name,
                    "description": p.description,
                    "arguments": p.arguments.iter().map(|(name, description, required)| json!({
                        "name": name, "description": description, "required": required,
                    })).collect::<Vec<_>>(),
                })).collect();
                for k in folio_control::harness::skills() {
                    list.push(json!({
                        "name": format!("skill-{}", k.name),
                        "title": k.title,
                        "description": format!("Skill: {}", k.when),
                        "arguments": [{ "name": "request", "description": "What the person wants, in their words", "required": false }],
                    }));
                }
                Ok(json!({ "prompts": list }))
            }
            "prompts/get" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "prompts/get needs `name`".to_string()))?;
                if let Some(skill) = name.strip_prefix("skill-") {
                    let k = folio_control::harness::skill(skill).map_err(|e| (-32602, e))?;
                    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                    let request = prompts::arg(&arguments, "request", "");
                    let text = if request.is_empty() {
                        format!("Follow this folio skill, then the finish routine (harness_check, harness_look, fix, report).\n\n{}", k.markdown)
                    } else {
                        format!("{request}\n\nFollow this folio skill, then the finish routine (harness_check, harness_look, fix, report).\n\n{}", k.markdown)
                    };
                    return Ok(json!({ "description": k.when, "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] }));
                }
                let prompt = prompts::PROMPTS.iter().find(|p| p.name == name).ok_or((-32602, format!("Unknown prompt `{name}`")))?;
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                for (arg, _, required) in prompt.arguments {
                    if *required && prompts::arg(&arguments, arg, "").is_empty() {
                        return Err((-32602, format!("Prompt `{name}` needs `{arg}`")));
                    }
                }
                Ok(json!({
                    "description": prompt.description,
                    "messages": [{ "role": "user", "content": { "type": "text", "text": (prompt.render)(&arguments) } }],
                }))
            }
            "completion/complete" => Ok(json!({ "completion": { "values": [] } })),
            _ => Err((-32601, format!("Method not found: {method}"))),
        }
    }

    fn instructions(&self) -> String {
        let mode = match &self.backend {
            Backend::Live { .. } => "Live mode: every tool runs in the open folio window. The person may be editing at the same time; you share one undo history (history_undo undoes the last step, whoever made it).".to_string(),
            Backend::Local { file: Some(p), .. } => format!("File mode on {}: the file is saved after every change. Commands that need the window (ui_*, presenting) are unavailable.", p.display()),
            Backend::Local { .. } => "Headless mode (the app isn't running): file_new or file_open first.".into(),
        };
        if for_builtin_agent() {
            // The built-in agent has the brief in its system prompt already.
            return format!("folio's MCP server for its built-in agent. {mode}");
        }
        format!(
            "{mode}\nEach tool is one folio command (family_verb is family.verb in the brief below). Results end with a fresh <context> block when the file changed (with what the person changed meanwhile) and, after an edit, \
             a reminder of the finish routine until you run harness_check or harness_look; these notes are also under harnessNotes in the structured result. \
             Skills are also prompts (skill-<name>) and resources (folio://skills/<name>).\n\n{}",
            folio_control::harness::brief()
        )
    }

    fn new(backend: Backend) -> Self {
        Server { backend, context: Mutex::new(None), unchecked: Mutex::new(false) }
    }

    /// What the harness adds to a tool's result: where an edit was saved, the live context when
    /// it changed, and the finish routine's reminder after an edit until the agent checks.
    async fn notes(&self, spec: &registry::Spec, ok: bool) -> Vec<String> {
        let mut notes = vec![];
        if ok
            && spec.mutates
            && let Some(path) = self.backend.path()
        {
            notes.push(format!("(saved {})", path.display()));
        }
        if spec.family() != "harness"
            && let Some(block) = self.fresh_context().await
        {
            notes.push(block);
        }
        let mut unchecked = self.unchecked.lock().unwrap_or_else(|e| e.into_inner());
        if ok && matches!(spec.name, "harness.look" | "harness.check") {
            *unchecked = false;
        } else if ok && spec.mutates && spec.family() != "harness" {
            *unchecked = true;
        }
        if *unchecked {
            notes.push(NOT_CHECKED.into());
        }
        notes
    }

    /// The live context, when it changed since the last one given: as a `<context>` block.
    async fn fresh_context(&self) -> Option<String> {
        let since = self.context.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|(seq, _)| *seq);
        let v = self.backend.call("harness.context", json!({ "since": since })).await.ok()?;
        let text = v["text"].as_str()?.to_string();
        let seq = v["seq"].as_u64().unwrap_or(0);
        let mut last = self.context.lock().unwrap_or_else(|e| e.into_inner());
        let changed = last.as_ref().is_none_or(|(_, t)| *t != text);
        *last = Some((seq, text.clone()));
        changed.then(|| format!("<context>\n{text}\n</context>"))
    }
}

/// Registry-backed resources: uri, name, description, command.
const RESOURCES: [(&str, &str, &str, &str); 5] = [
    ("folio://file/overview", "File overview", "Read it first: the whole open file in one bounded answer.", "file.overview"),
    ("folio://file", "Open file", "The complete open file as JSON (the .folio format's document.json).", "file.get"),
    ("folio://commands", "Commands", "Every command with its parameters, permission and whether it needs the window.", "app.commands"),
    ("folio://settings", "Settings", "folio's settings: agent permissions, appearance, editing.", "app.settings"),
    ("folio://app", "Application", "Version, folders, and whether the window and the bridge are running.", "app.info"),
];

/// Started by the app's built-in agent (Claude Code or Codex as the panel's model): it gets no
/// `agent_*` tools, which would drive the agent itself.
fn for_builtin_agent() -> bool {
    std::env::var_os("FOLIO_MCP_BUILTIN_AGENT").is_some_and(|v| !v.is_empty() && v != "0")
}

/// One tool per command an agent can run (person-only commands are left out: they are always refused).
fn tools() -> Vec<Value> {
    let builtin = for_builtin_agent();
    registry::commands()
        .iter()
        .filter(|s| s.perm != Perm::PersonOnly && !(builtin && s.family() == "agent"))
        .map(|spec| {
            let mut description = spec.doc.to_string();
            if spec.perm != Perm::Edit {
                description.push_str(&format!(
                    " Needs the \"{}\" agent permission (Settings › Agent › Permissions).",
                    spec.perm.label()
                ));
            }
            if spec.needs_window {
                description.push_str(" Needs the running folio window (folio-mcp --live).");
            }
            let destructive = spec.mutates && ["delete", "remove", "close", "quit", "revertTo", "open", "new", "clear", "replace"].iter().any(|w| spec.name.to_lowercase().contains(&w.to_lowercase()));
            json!({
                "name": spec.tool_name(),
                "title": spec.name,
                "description": description,
                "inputSchema": registry::input_schema(spec),
                "annotations": {
                    "readOnlyHint": !spec.mutates,
                    "destructiveHint": destructive,
                    "idempotentHint": !spec.mutates,
                    "openWorldHint": matches!(spec.family(), "handoff" | "account") || matches!(spec.name, "app.checkUpdates" | "agent.send"),
                },
            })
        })
        .collect()
}

/// A `tools/call` result. The notes end the text and also go under `harnessNotes` in
/// `structuredContent`, because some clients (Claude Code) show the structured result instead of
/// the text when both are there. A result with pictures has no `structuredContent`, so such a
/// client still gets the pictures (the text carries the same JSON).
fn tool_reply(result: Result<Value, String>, notes: Vec<String>, pictures: Vec<Value>) -> Value {
    let joined = |text: String| if notes.is_empty() { text } else { format!("{text}\n\n{}", notes.join("\n\n")) };
    match result {
        Ok(value) => {
            // Markdown answers (the brief, a skill, a guide) go as they are.
            let text = match &value {
                Value::String(s) => s.clone(),
                _ => serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
            };
            let mut content = vec![json!({ "type": "text", "text": joined(text) })];
            let pictured = !pictures.is_empty();
            content.extend(pictures);
            let mut reply = json!({ "content": content, "isError": false });
            if let Value::Object(mut object) = value
                && !pictured
            {
                if !notes.is_empty() {
                    object.insert("harnessNotes".into(), json!(notes));
                }
                reply["structuredContent"] = Value::Object(object);
            }
            reply
        }
        Err(message) => json!({ "content": [{ "type": "text", "text": joined(message) }], "isError": true }),
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_ride_in_the_text_and_the_structured_result() {
        let notes = vec!["(saved /tmp/a.folio)".to_string(), NOT_CHECKED.to_string()];
        let r = tool_reply(Ok(json!({ "changed": 1 })), notes.clone(), vec![]);
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("{") && text.ends_with(NOT_CHECKED), "{text}");
        assert_eq!(r["structuredContent"]["changed"], 1);
        assert_eq!(r["structuredContent"]["harnessNotes"], json!(notes));
        // Markdown and lists have no structured result: the text says it all.
        let r = tool_reply(Ok(json!("# Brief")), vec![], vec![]);
        assert_eq!((r["content"][0]["text"].as_str(), r.get("structuredContent")), (Some("# Brief"), None));
        // Without notes, the structured result is the command's answer as it is.
        let r = tool_reply(Ok(json!({ "a": 1 })), vec![], vec![]);
        assert_eq!(r["structuredContent"], json!({ "a": 1 }));
        // Errors carry the notes in their text.
        let r = tool_reply(Err("No such page.".into()), vec!["<context>\nx\n</context>".into()], vec![]);
        assert_eq!(r["isError"], true);
        assert_eq!(r["content"][0]["text"], "No such page.\n\n<context>\nx\n</context>");
    }

    /// Over a real file: an edit's result carries the saved line, the context and the finish
    /// routine's reminder in `harnessNotes`, until the agent checks; a look's picture arrives.
    #[tokio::test(flavor = "multi_thread")]
    async fn harness_notes_reach_clients_that_show_the_structured_result() {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: set before any session starts; no other test in this binary reads them.
        unsafe {
            std::env::set_var("FOLIO_DATA_DIR", dir.path().join("data"));
            std::env::set_var("FOLIO_CONFIG_DIR", dir.path().join("config"));
            std::env::set_var("FOLIO_CONTROL", dir.path().join("control.json"));
            std::env::set_var("LSUITE_HOME", dir.path().join("lsuite"));
        }
        let path = dir.path().join("notes.folio");
        let server = Server::new(Backend::file(&path, Source::Mcp).await.unwrap());
        let call = async |name: &str, arguments: Value| server.dispatch("tools/call", &json!({ "name": name, "arguments": arguments })).await.unwrap();
        let notes = |r: &Value| r["structuredContent"]["harnessNotes"].as_array().cloned().unwrap_or_default().iter().map(|n| n.as_str().unwrap().to_string()).collect::<Vec<_>>();

        let made = call("file_new", json!({ "kind": "sheet", "title": "Notes" })).await;
        assert_eq!(made["isError"], false, "{made}");
        let set = call("sheet_setRange", json!({ "at": "A1", "values": [["Item", "Cost"], ["Rent", 1200], ["Food", 300], ["Total", "=SUM(B2:B3)"]] })).await;
        assert_eq!(set["isError"], false, "{set}");
        let n = notes(&set);
        assert!(n.iter().any(|n| n.starts_with("(saved ")), "{n:?}");
        assert!(n.iter().any(|n| n.starts_with("<context>") && n.contains("1 formula")), "{n:?}");
        assert_eq!(n.last().map(String::as_str), Some(NOT_CHECKED));
        assert!(set["content"][0]["text"].as_str().unwrap().ends_with(NOT_CHECKED), "the text has the notes too");
        // Reading doesn't end the reminder; checking does.
        assert!(notes(&call("sheet_read", json!({ "range": "A1:B4" })).await).iter().any(|n| n == NOT_CHECKED));
        let checked = call("harness_check", json!({})).await;
        assert_eq!(checked["isError"], false, "{checked}");
        assert!(!checked.to_string().contains("Not checked yet"), "{checked}");
        assert!(!call("sheet_read", json!({ "range": "A1:B4" })).await.to_string().contains("Not checked yet"));
        // A look: the picture follows the text, with no structured result to hide it.
        call("sheet_set", json!({ "cell": "A5", "value": "Note" })).await;
        let look = call("harness_look", json!({ "range": "A1:B5" })).await;
        assert_eq!(look["isError"], false, "{look}");
        assert_eq!((look["content"][1]["type"].as_str(), look["content"][1]["mimeType"].as_str()), (Some("image"), Some("image/png")), "{look}");
        assert!(look.get("structuredContent").is_none());
        assert!(!look.to_string().contains("Not checked yet"), "looking ends the reminder");
        // An error keeps its message, with the notes after it.
        let wrong = call("sheet_read", json!({ "page": "Nope" })).await;
        assert_eq!(wrong["isError"], true, "{wrong}");
    }

    #[tokio::test]
    async fn lines_are_read_within_the_limit() {
        let input: &[u8] = b"{\"a\":1}\r\n0123456789abcdef\nshort\nlast";
        let mut r = tokio::io::BufReader::with_capacity(4, input);
        assert_eq!(read_line(&mut r, 10).await.unwrap(), Line::Text("{\"a\":1}".into()));
        // Skipped to its end, and the next line is whole.
        assert_eq!(read_line(&mut r, 10).await.unwrap(), Line::TooLong);
        assert_eq!(read_line(&mut r, 10).await.unwrap(), Line::Text("short".into()));
        assert_eq!(read_line(&mut r, 10).await.unwrap(), Line::Text("last".into()));
        assert_eq!(read_line(&mut r, 10).await.unwrap(), Line::Eof);
    }
}
