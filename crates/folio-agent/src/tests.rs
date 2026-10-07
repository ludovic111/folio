use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use folio_control::{Session, SessionOptions, Source};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use crate::*;

/// Every test runs with its own lsuite home (never the person's account), set once before any
/// session exists.
fn lsuite_home() -> &'static std::path::Path {
    static HOME: OnceLock<tempfile::TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: set once, before any test thread reads it (every test starts by calling this).
        unsafe {
            std::env::set_var("LSUITE_HOME", dir.path());
            std::env::remove_var("LSUITE_ACCOUNT_SERVER");
        }
        dir
    })
    .path()
}

/// lsuite tests write and remove the one account file: one at a time.
async fn account_lock() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Default::default).lock().await
}

fn sign_in(server: &str, token: &str) {
    lsuite_home();
    folio_control::account::write(&folio_control::account::AccountFile {
        format: 1,
        server: server.into(),
        email: "ada@example.com".into(),
        name: "Ada".into(),
        plan: "pro".into(),
        token: token.into(),
        signed_in_at: chrono::Utc::now(),
    })
    .unwrap();
}

fn sign_out() {
    lsuite_home();
    let _ = std::fs::remove_file(folio_control::account::path());
}

fn session(dir: &std::path::Path) -> Arc<Session> {
    lsuite_home();
    Session::new(SessionOptions { data_dir: Some(dir.join("data")), config_dir: Some(dir.join("config")), secrets: None, headless: true }).unwrap()
}

/// A session with a new file whose first page is a sheet.
pub(crate) async fn with_sheet(dir: &std::path::Path) -> Arc<Session> {
    let s = session(dir);
    folio_control::call(&s, Source::Window, "file.new", json!({ "title": "Test", "kind": "sheet" })).await.unwrap();
    s
}

async fn cells(s: &Arc<Session>) -> Value {
    folio_control::call(s, Source::Window, "sheet.read", json!({})).await.unwrap()["rows"].clone()
}

/// Answers each request with the next body, in order (the last one repeats).
struct Script {
    bodies: Vec<String>,
    content_type: &'static str,
    next: AtomicUsize,
}

impl Respond for Script {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        let i = self.next.fetch_add(1, Ordering::SeqCst).min(self.bodies.len() - 1);
        ResponseTemplate::new(200).insert_header("content-type", self.content_type).set_body_string(self.bodies[i].clone())
    }
}

async fn mock(route: &str, content_type: &'static str, bodies: Vec<String>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path(route)).respond_with(Script { bodies, content_type, next: AtomicUsize::new(0) }).mount(&server).await;
    server
}

fn sse(events: &[Value]) -> String {
    events.iter().map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap_or("message"))).collect()
}

fn openai_sse(chunks: &[Value]) -> String {
    let mut s: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    s.push_str("data: [DONE]\n\n");
    s
}

/// Every event of a run, up to and including the terminal one.
async fn collect(run: &mut AgentRun) -> Vec<AgentEvent> {
    let mut out = vec![];
    tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(e) = run.next_event().await {
            out.push(e);
        }
    })
    .await
    .expect("the run finished");
    out
}

/// The events a panel draws, without status lines and token counts.
fn visible(events: &[AgentEvent]) -> Vec<&AgentEvent> {
    events.iter().filter(|e| !matches!(e, AgentEvent::Status { .. } | AgentEvent::Usage { .. })).collect()
}

fn anthropic_text(text: &str) -> String {
    sse(&[
        json!({ "type": "message_start", "message": { "id": "msg_2", "role": "assistant", "content": [], "usage": { "input_tokens": 200, "output_tokens": 1 } } }),
        json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }),
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": text } }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" }, "usage": { "output_tokens": 8 } }),
        json!({ "type": "message_stop" }),
    ])
}

/// One Anthropic answer that calls a tool.
fn anthropic_tool(id: &str, name: &str, input: &str) -> String {
    sse(&[
        json!({ "type": "message_start", "message": { "id": "msg_t", "role": "assistant", "content": [], "usage": { "input_tokens": 100, "output_tokens": 1 } } }),
        json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "tool_use", "id": id, "name": name, "input": {} } }),
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "input_json_delta", "partial_json": input } }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 20 } }),
        json!({ "type": "message_stop" }),
    ])
}

#[tokio::test(flavor = "multi_thread")]
async fn anthropic_tool_call_lands_in_the_file_and_the_run_reverts() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    s.set_secret("anthropic", Some("sk-ant-test")).unwrap();
    let first = sse(&[
        json!({ "type": "message_start", "message": { "id": "msg_1", "role": "assistant", "content": [], "usage": { "input_tokens": 120, "output_tokens": 1 } } }),
        json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "thinking", "thinking": "" } }),
        json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "signature_delta", "signature": "sig-abc" } }),
        json!({ "type": "content_block_stop", "index": 0 }),
        json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "text", "text": "" } }),
        json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": "Filling the sheet." } }),
        json!({ "type": "content_block_stop", "index": 1 }),
        json!({ "type": "content_block_start", "index": 2, "content_block": { "type": "tool_use", "id": "toolu_1", "name": "sheet_setRange", "input": {} } }),
        json!({ "type": "content_block_delta", "index": 2, "delta": { "type": "input_json_delta", "partial_json": "{\"at\": \"A1\", \"values\": [[\"Tea\", 3], [\"Cake\", 4.5], " } }),
        json!({ "type": "content_block_delta", "index": 2, "delta": { "type": "input_json_delta", "partial_json": "[\"Total\", \"=SUM(B1:B2)\"]]}" } }),
        json!({ "type": "content_block_stop", "index": 2 }),
        json!({ "type": "message_delta", "delta": { "stop_reason": "tool_use" }, "usage": { "output_tokens": 40 } }),
        json!({ "type": "message_stop" }),
    ]);
    let server = mock("/v1/messages", "text/event-stream", vec![first, anthropic_text("The total is 7.5."), anthropic_text("Sure.")]).await;
    let config = AgentConfig { base_url: server.uri(), ..AgentConfig::new(ProviderKind::Anthropic) };

    let mut run = Agent::start(&s, config.clone(), "Add tea and cake with a total", Conversation::new());
    let events = collect(&mut run).await;
    let shown = visible(&events);
    assert_eq!(shown.len(), 4, "{shown:#?}");
    assert!(matches!(shown[0], AgentEvent::Text { delta } if delta == "Filling the sheet."));
    match shown[1] {
        AgentEvent::Command { record, result } => {
            assert_eq!(record.command, "sheet.setRange");
            assert_eq!(record.source, Source::Agent);
            assert!(record.ok && record.mutates);
            assert!(record.seq > 0);
            assert!(result.is_some());
        }
        e => panic!("expected a command card, got {e:?}"),
    }
    assert!(matches!(shown[2], AgentEvent::Text { delta } if delta == "\n\nThe total is 7.5."));
    let (checkpoint, conversation) = match shown[3] {
        AgentEvent::Done { summary, checkpoint, changes, conversation } => {
            assert_eq!(summary, "The total is 7.5.");
            assert_eq!(*changes, 1);
            (checkpoint.expect("a checkpoint before the change"), conversation.clone())
        }
        e => panic!("expected Done, got {e:?}"),
    };
    assert_eq!(cells(&s).await[2][1], json!(7.5));
    assert_eq!(run.checkpoint(), Some(checkpoint));

    // What the model was sent back: the thinking block verbatim, then the tool result.
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].headers.get("x-api-key").unwrap(), "sk-ant-test");
    assert!(requests[0].headers.get("authorization").is_none(), "the Anthropic API gets the key once");
    let first: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(first["system"].as_str().unwrap().contains("file_overview"), "the agent is told to read the overview first");
    let second: Value = serde_json::from_slice(&requests[1].body).unwrap();
    assert_eq!(second["messages"][1]["content"][0], json!({ "type": "thinking", "thinking": "", "signature": "sig-abc" }));
    assert_eq!(second["messages"][1]["content"][2]["input"]["at"], "A1");
    let result = &second["messages"][2]["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["tool_use_id"], "toolu_1");
    assert_eq!(result["is_error"], false);
    assert_eq!(second["tools"].as_array().unwrap().len(), tool_defs().len());

    // A follow-up continues the thread, with what the person sees in front of the request.
    let mut next = Agent::start(&s, config, "Thanks", conversation);
    let events = collect(&mut next).await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { summary, checkpoint: None, changes: 0, .. }) if summary == "Sure."));
    let third: Value = serde_json::from_slice(&server.received_requests().await.unwrap()[2].body).unwrap();
    assert_eq!(third["messages"].as_array().unwrap().len(), 5);
    let followup = third["messages"][4]["content"][0]["text"].as_str().unwrap();
    assert!(followup.starts_with("<context>\n") && followup.contains("File \"Test\"") && followup.ends_with("\n</context>\n\nThanks"), "{followup}");

    // Revert this run.
    revert(&s, checkpoint).await.unwrap();
    assert_eq!(cells(&s).await, json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denied_permission_goes_back_to_the_model_as_a_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    s.set_secret("openai", Some("sk-test")).unwrap();
    let first = openai_sse(&[
        json!({ "choices": [{ "index": 0, "delta": { "role": "assistant", "content": "Turning off update checks." } }] }),
        json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "id": "call_1", "type": "function", "function": { "name": "app_setSetting", "arguments": "" } }] } }] }),
        json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": "{\"key\":\"updates.checkOnStart\",\"value\":false}" } }] } }] }),
        json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }] }),
    ]);
    let second = openai_sse(&[
        json!({ "choices": [{ "index": 0, "delta": { "content": "I can't: the settings permission is off." } }] }),
        json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] }),
    ]);
    let server = mock("/chat/completions", "text/event-stream", vec![first, second]).await;
    let config = AgentConfig { base_url: server.uri(), ..AgentConfig::new(ProviderKind::OpenAi) };
    let mut run = Agent::start(&s, config, "Stop checking for updates", Conversation::new());
    let events = collect(&mut run).await;
    let shown = visible(&events);
    assert_eq!(shown.len(), 4, "{shown:#?}");
    match shown[1] {
        AgentEvent::Command { record, result } => {
            assert_eq!(record.command, "app.setSetting");
            assert!(!record.ok);
            assert!(record.error.as_deref().unwrap().contains("\"settings\" permission"), "{record:?}");
            assert!(result.is_none());
        }
        e => panic!("expected a command card, got {e:?}"),
    }
    assert!(matches!(shown[3], AgentEvent::Done { checkpoint: None, changes: 0, .. }), "{:?}", shown[3]);
    assert!(s.settings().updates.check_on_start);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].headers.get("authorization").unwrap(), "Bearer sk-test");
    let body: Value = serde_json::from_slice(&requests[1].body).unwrap();
    let tool = body["messages"].as_array().unwrap().iter().find(|m| m["role"] == "tool").expect("a tool message");
    assert_eq!(tool["tool_call_id"], "call_1");
    assert!(tool["content"].as_str().unwrap().contains("permission"), "{tool}");
    assert_eq!(body["messages"][0]["role"], "system");
    // OpenAI takes at most 128 tools: the core set, and folio_run for the rest.
    let tools = body["tools"].as_array().unwrap();
    assert!(tools.len() <= 128, "{}", tools.len());
    assert_eq!(tools.len() < tool_defs().len(), tools.iter().any(|t| t["function"]["name"] == RUN_TOOL));
}

#[tokio::test(flavor = "multi_thread")]
async fn ollama_runs_tools_locally() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    let first = [
        json!({ "message": { "role": "assistant", "content": "", "tool_calls": [{ "function": { "name": "sheet_set", "arguments": { "cell": "B2", "value": "=6*7" } } }] }, "done": false }),
        json!({ "message": { "role": "assistant", "content": "" }, "done": true, "done_reason": "stop", "prompt_eval_count": 10, "eval_count": 5 }),
    ]
    .map(|v| v.to_string())
    .join("\n");
    let second = json!({ "message": { "role": "assistant", "content": "It shows 42." }, "done": true }).to_string();
    let server = mock("/api/chat", "application/x-ndjson", vec![first, second]).await;
    let config = AgentConfig { base_url: server.uri(), model: "qwen3".into(), ..AgentConfig::new(ProviderKind::Ollama) };
    let mut run = Agent::start(&s, config, "Put 6×7 in B2", Conversation::new());
    let events = collect(&mut run).await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { changes: 1, summary, .. }) if summary == "It shows 42."), "{events:#?}");
    assert_eq!(cells(&s).await[1][1], json!(42.0));
    let requests = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&requests[1].body).unwrap();
    let tool = body["messages"].as_array().unwrap().iter().find(|m| m["role"] == "tool").unwrap();
    assert_eq!(tool["tool_name"], "sheet_set");
    // A small local model gets the short list.
    assert!(body["tools"].as_array().unwrap().len() < 30);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_stops_a_run_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    s.set_secret("anthropic", Some("k")).unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(60))).mount(&server).await;
    let config = AgentConfig { base_url: server.uri(), ..AgentConfig::new(ProviderKind::Anthropic) };
    let mut run = Agent::start(&s, config, "Do something slow", Conversation::new());
    assert!(matches!(run.next_event().await, Some(AgentEvent::Status { .. })));
    run.cancel();
    let events = tokio::time::timeout(Duration::from_secs(5), collect(&mut run)).await.expect("cancelled promptly");
    assert!(matches!(events.last(), Some(AgentEvent::Cancelled { checkpoint: None, changes: 0 })), "{events:?}");
    assert!(run.is_finished());
    // The thread keeps the request, so a follow-up has the context.
    assert_eq!(run.conversation().messages.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_keys_and_disabled_agents_are_explained() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    // Point at a dead address so a key from the environment can't reach anything.
    let config = AgentConfig { base_url: "http://127.0.0.1:9".into(), ..AgentConfig::new(ProviderKind::Anthropic) };
    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        let mut run = Agent::start(&s, config.clone(), "Hi", Conversation::new());
        let events = collect(&mut run).await;
        assert!(matches!(events.last(), Some(AgentEvent::Error { message, .. }) if message.contains("No Anthropic API key")), "{events:?}");
    }
    s.update_settings(|st| st.agent.permissions.enabled = false).unwrap();
    let mut run = Agent::start(&s, config, "Hi", Conversation::new());
    let events = collect(&mut run).await;
    assert!(matches!(events.last(), Some(AgentEvent::Error { message, .. }) if message.contains("turned off")), "{events:?}");
}

#[test]
fn tools_cover_the_registry_except_person_only_commands() {
    let defs = tool_defs();
    assert!(defs.iter().any(|t| t.name == "file_overview"));
    assert!(defs.iter().all(|t| t.name != "app_setAgentKey" && t.name != "account_signIn" && !t.name.starts_with("agent_")));
    let mut names: Vec<&str> = defs.iter().map(|t| t.name.as_str()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), defs.len());
    for t in &defs {
        assert!(t.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && t.name.len() <= 64, "{}", t.name);
        assert_eq!(t.schema["type"], "object");
        assert_eq!(tools::spec_for_tool(&t.name).unwrap().name, t.command);
    }
    assert_eq!(tools::spec_for_tool("mcp__folio__sheet_setRange").unwrap().name, "sheet.setRange");
    let long = "é".repeat(TOOL_OUTPUT_LIMIT);
    let (out, err) = tools::tool_output(&Ok(json!(long)));
    assert!(!err && out.contains("truncated") && out.len() < TOOL_OUTPUT_LIMIT + 200);
}

#[test]
fn trimmed_tool_sets_keep_the_core_and_name_real_commands() {
    let set = ToolSet::new(Some(128), false);
    assert!(set.defs.len() <= 128);
    if set.trimmed {
        assert_eq!(set.defs.last().unwrap().name, RUN_TOOL);
        assert!(set.system_prompt().contains(RUN_TOOL));
    }
    assert!(set.defs.iter().any(|t| t.name == "file_overview"));
    let compact = ToolSet::new(None, true);
    assert!(compact.defs.len() < 30);
    assert!(!ToolSet::new(None, false).trimmed);
    // Every command named in the lists exists.
    for name in tools_lists() {
        assert!(folio_control::spec(name).is_some(), "{name} isn't a command");
    }
}

fn tools_lists() -> Vec<&'static str> {
    let core = ToolSet::new(Some(1000), true).defs.into_iter().chain(ToolSet::new(Some(100), false).defs);
    core.filter(|t| t.name != RUN_TOOL).map(|t| t.command).collect()
}

#[test]
fn settings_choose_the_provider() {
    let mut a = folio_control::settings::AgentSettings::default();
    assert_eq!(AgentConfig::from_settings(&a).provider, ProviderKind::Lsuite, "lsuite AI is the default");
    a.provider = "anthropic".into();
    let c = AgentConfig::from_settings(&a);
    assert_eq!((c.provider, c.model(), c.base_url()), (ProviderKind::Anthropic, "claude-sonnet-5-5".to_string(), "https://api.anthropic.com".to_string()));
    a.provider = "ollama".into();
    a.base_url = "http://box:11434/".into();
    assert_eq!(AgentConfig::from_settings(&a).base_url(), "http://box:11434");
    a.provider = "nonsense".into();
    assert_eq!(AgentConfig::from_settings(&a).provider, ProviderKind::Lsuite);
    assert_eq!(serde_json::to_value(ProviderKind::OpenAi).unwrap(), "openai");
    assert_eq!(ProviderKind::parse("Google"), Some(ProviderKind::Gemini));
    assert_eq!(ProviderKind::parse("lm studio"), Some(ProviderKind::LmStudio));
    assert_eq!(ProviderKind::parse("lsuite ai"), Some(ProviderKind::Lsuite));
    // The keys the agent reads are the ones app.setAgentKey saves.
    for kind in ProviderKind::ALL {
        if let Some(k) = kind.info().key.filter(|k| k.required) {
            assert!(status::SAVABLE_KEYS.contains(&k.id), "{kind}: its key can't be saved");
        }
    }
}

#[test]
fn a_stopped_turn_is_closed_before_the_next() {
    let mut c = Conversation::new();
    c.messages.push(Message::user("go"));
    c.messages.push(Message {
        role: Role::Assistant,
        parts: vec![
            Part::ToolUse { id: "a".into(), name: "page_list".into(), input: json!({}) },
            Part::ToolUse { id: "b".into(), name: "page_list".into(), input: json!({}) },
        ],
    });
    c.messages.push(Message { role: Role::User, parts: vec![Part::ToolResult { id: "a".into(), name: "page_list".into(), output: "[]".into(), is_error: false }] });
    c.prepare_turn();
    assert_eq!(c.messages.len(), 3);
    assert!(matches!(&c.messages[2].parts[1], Part::ToolResult { id, is_error: true, .. } if id == "b"));
}

#[test]
fn cli_invocations_attach_folio_only() {
    let args = cli::claude_args(std::path::Path::new("/tmp/mcp.json"), "", Some("sess-1"), false);
    let joined = args.join(" ");
    for flag in ["-p", "--output-format stream-json", "--strict-mcp-config", "--allowedTools mcp__folio__*", "--mcp-config /tmp/mcp.json", "--resume sess-1"] {
        assert!(joined.contains(flag), "{flag} in {joined}");
    }
    assert!(!joined.contains("--model"));
    let live = cli::Live { mcp: "/Apps/folio \"x\"/folio-mcp".into(), control: "/data/control.json".into() };
    let config = cli::mcp_config(&live);
    assert_eq!(config["mcpServers"]["folio"]["args"], json!(["--live"]));
    assert_eq!(config["mcpServers"]["folio"]["env"]["FOLIO_MCP_BUILTIN_AGENT"], "1", "folio-mcp hides its agent_* tools");
    assert_eq!(config["mcpServers"]["folio"]["env"]["FOLIO_CONTROL"], "/data/control.json");
    let args = cli::codex_args(&live, "gpt-5", Some("thread-9"), &["node_repl".into(), "folio".into()]);
    assert_eq!(&args[..2], ["exec", "--json"]);
    for c in ["features.plugins=false", "features.computer_use=false", "mcp_servers.node_repl.enabled=false", "mcp_servers.folio.env.FOLIO_MCP_BUILTIN_AGENT=\"1\""] {
        assert!(args.windows(2).any(|w| w == ["-c", c]), "{c} in {args:?}");
    }
    assert!(!args.iter().any(|a| a == "mcp_servers.folio.enabled=false"));
    assert!(args.windows(2).any(|w| w == ["-c", "mcp_servers.folio.default_tools_approval_mode=\"approve\""]), "Codex runs folio's tools without asking");
    assert!(args.contains(&"mcp_servers.folio.command=\"/Apps/folio \\\"x\\\"/folio-mcp\"".to_string()), "{args:?}");
    assert!(args.windows(2).any(|w| w == ["resume", "thread-9"]));
    assert_eq!(args.last().unwrap(), "-");
    // A Windows shim gets the system prompt on one line.
    let args = cli::claude_args(std::path::Path::new("/tmp/mcp.json"), "", None, true);
    let system = &args[args.iter().position(|a| a == "--append-system-prompt").unwrap() + 1];
    assert!(!system.contains('\n') && system.contains("mcp__folio__family_verb"), "{system}");
}

#[test]
fn codex_config_servers_are_found() {
    let config = r#"
model = "gpt-5"
[mcp_servers.node_repl]
command = "node"
[mcp_servers.node_repl.env]
X = "1"
[mcp_servers."my.server"]
command = "x"
[projects."/home/me"]
trust_level = "trusted"
[mcp_servers]
inline = { command = "y" }
"#;
    assert_eq!(cli::codex_mcp_servers(config), ["node_repl", "\"my.server\"", "inline"]);
}

#[test]
fn children_get_a_path_with_the_cli_folder_first() {
    let path = cli::child_path(std::path::Path::new("/opt/somewhere/bin/claude"));
    let dirs: Vec<_> = std::env::split_paths(&path).collect();
    assert_eq!(dirs[0], std::path::Path::new("/opt/somewhere/bin"));
    for d in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).filter(|d| !d.as_os_str().is_empty()) {
        assert!(dirs.contains(&d), "{d:?} kept");
    }
}

#[cfg(unix)]
fn script(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("fake-cli");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn claude_code_stream_json_becomes_events() {
    let dir = tempfile::tempdir().unwrap();
    let s = session(dir.path());
    let lines = [
        json!({ "type": "system", "subtype": "init", "session_id": "sess-1", "mcp_servers": [{ "name": "folio", "status": "connected" }] }),
        json!({ "type": "stream_event", "event": { "type": "message_start" } }),
        json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Hel" } } }),
        json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "lo" } } }),
        json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "Hello" }] } }),
        json!({ "type": "result", "subtype": "success", "is_error": false, "result": "Hello", "session_id": "sess-1", "usage": { "input_tokens": 3, "output_tokens": 2 } }),
    ];
    let echo: String = lines.iter().map(|l| format!("echo '{l}'\n")).collect();
    let got = dir.path().join("prompt.txt");
    let exe = script(dir.path(), &format!("cat > '{}'\n{echo}", got.display()));
    let (mut run, mut handle) = Run::new(&s, AgentConfig::new(ProviderKind::ClaudeCode), &Conversation::new());
    let (out, result) = cli::run_child(&mut run, tokio::process::Command::new(exe), "the prompt".into(), "Claude Code", cli::parse_claude).await;
    result.unwrap();
    assert_eq!((out.reply.as_str(), out.session_id.as_deref()), ("Hello", Some("sess-1")));
    assert_eq!(std::fs::read_to_string(got).unwrap(), "the prompt");
    drop(run);
    let mut deltas = vec![];
    while let Some(e) = handle.next_event().await {
        if let AgentEvent::Text { delta } = e {
            deltas.push(delta);
        }
    }
    assert_eq!(deltas, ["Hel", "lo"]);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn cancelling_kills_the_cli_and_its_children() {
    let dir = tempfile::tempdir().unwrap();
    let s = session(dir.path());
    let pid_file = dir.path().join("pid");
    let exe = script(dir.path(), &format!("sleep 60 &\necho $! > '{}'\nwait", pid_file.display()));
    let (mut run, _handle) = Run::new(&s, AgentConfig::new(ProviderKind::ClaudeCode), &Conversation::new());
    let child = cli::run_child(&mut run, tokio::process::Command::new(exe), String::new(), "Claude Code", cli::parse_claude);
    assert!(tokio::time::timeout(Duration::from_millis(800), child).await.is_err(), "the fake CLI waits");
    let pid = std::fs::read_to_string(&pid_file).unwrap().trim().to_string();
    let alive = || std::process::Command::new("kill").args(["-0", &pid]).stderr(std::process::Stdio::null()).status().unwrap().success();
    for _ in 0..40 {
        if !alive() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the CLI's child {pid} survived the cancel");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cli_without_folio_tools_stops_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let s = session(dir.path());
    let init = json!({ "type": "system", "subtype": "init", "session_id": "s", "mcp_servers": [{ "name": "folio", "status": "failed" }] });
    let exe = script(dir.path(), &format!("echo '{init}'\nsleep 60"));
    let (mut run, _handle) = Run::new(&s, AgentConfig::new(ProviderKind::ClaudeCode), &Conversation::new());
    let child = cli::run_child(&mut run, tokio::process::Command::new(exe), String::new(), "Claude Code", cli::parse_claude);
    let (_, result) = tokio::time::timeout(Duration::from_secs(5), child).await.expect("stopped at once");
    assert!(result.unwrap_err().contains("couldn't start folio-mcp"));
}

#[tokio::test(flavor = "multi_thread")]
async fn provider_status_says_what_is_usable() {
    let dir = tempfile::tempdir().unwrap();
    let s = session(dir.path());
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/tags"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "models": [{ "name": "qwen3:8b" }] })))
        .mount(&server)
        .await;
    s.update_settings(|st| {
        st.agent.provider = "ollama".into();
        st.agent.base_url = server.uri();
    })
    .unwrap();
    s.set_secret("openai", Some("sk-test")).unwrap();
    let all = provider_status(&s).await;
    assert_eq!(all.len(), ProviderKind::ALL.len());
    assert_eq!(all[0].provider, ProviderKind::Lsuite, "lsuite AI comes first");
    let get = |k| all.iter().find(|p| p.provider == k).unwrap();
    let ollama = get(ProviderKind::Ollama);
    assert!(ollama.ready && ollama.active, "{ollama:?}");
    assert_eq!(ollama.models, ["qwen3:8b"]);
    assert!(get(ProviderKind::OpenAi).ready);
    // No bridge in a headless session: the CLIs can't reach folio, whatever is installed.
    let claude = get(ProviderKind::ClaudeCode);
    assert!(!claude.ready && !claude.message.is_empty(), "{claude:?}");
}

/// A session with a sheet, the host installed (and a stand-in window: `agent.*` needs the app).
async fn hosted(dir: &std::path::Path, server: &MockServer) -> (Arc<Session>, Arc<Host>) {
    use futures::StreamExt;
    let s = with_sheet(dir).await;
    s.set_secret("anthropic", Some("sk-ant-test")).unwrap();
    s.update_settings(|st| {
        st.agent.provider = "anthropic".into();
        st.agent.base_url = server.uri();
    })
    .unwrap();
    let mut calls = s.attach_ui();
    tokio::spawn(async move { while calls.next().await.is_some() {} });
    let host = Host::install(&s);
    (s, host)
}

async fn call(s: &Arc<Session>, source: Source, name: &str, params: Value) -> Value {
    folio_control::call(s, source, name, params).await.unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// `agent.*` from the CLI: send and wait, read the run with its commands and the conversation,
/// revert it; the next request continues the thread.
#[tokio::test(flavor = "multi_thread")]
async fn clients_drive_the_agent_with_commands() {
    let dir = tempfile::tempdir().unwrap();
    let server = mock(
        "/v1/messages",
        "text/event-stream",
        vec![anthropic_tool("toolu_1", "sheet_set", "{\"cell\": \"A1\", \"value\": \"Hello\"}"), anthropic_text("Wrote Hello."), anthropic_text("You're welcome.")],
    )
    .await;
    let (s, host) = hosted(dir.path(), &server).await;
    // A command from a terminal before the run shows as a card of its own.
    call(&s, Source::Cli, "sheet.set", json!({ "cell": "C3", "value": "9" })).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while call(&s, Source::Cli, "agent.conversation", json!({})).await["entries"] == json!([]) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the terminal's card");

    let run = call(&s, Source::Cli, "agent.send", json!({ "prompt": "Write Hello in A1", "wait": true })).await;
    assert_eq!(run["state"], "done", "{run}");
    assert_eq!(run["source"], "cli");
    assert_eq!(run["reply"], "Wrote Hello.");
    assert_eq!(run["changes"], 1);
    assert_eq!(run["canRevert"], true);
    let list = run["commandList"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{run}");
    assert_eq!(list[0]["command"], "sheet.set");
    assert_eq!(cells(&s).await[0][0], "Hello");

    let status = call(&s, Source::Mcp, "agent.status", json!({})).await;
    assert_eq!(status["id"], run["id"]);
    assert_eq!(status["running"], Value::Null);
    let convo = call(&s, Source::Cli, "agent.conversation", json!({})).await;
    let kinds: Vec<&str> = convo["entries"].as_array().unwrap().iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["command", "user", "command", "assistant", "outcome"], "{convo}");
    assert_eq!(convo["entries"][0]["run"], Value::Null, "the terminal's edit is nobody's run");
    let next = convo["next"].as_u64().unwrap();
    assert_eq!(call(&s, Source::Cli, "agent.conversation", json!({ "since": next })).await["entries"], json!([]));
    assert_eq!(host.snapshot().entries.len(), 5);

    let reverted = call(&s, Source::Cli, "agent.revert", json!({})).await;
    assert_eq!(reverted["run"], run["id"]);
    let rows = cells(&s).await;
    assert_eq!(rows[0][0], Value::Null, "{rows}");
    assert_eq!(rows[2][2], json!(9.0), "only the run is reverted");
    let e = folio_control::call(&s, Source::Cli, "agent.revert", json!({})).await.unwrap_err();
    assert!(e.contains("No agent run to revert"), "{e}");
    // Redoing brings the run back, and it can be reverted again.
    call(&s, Source::Window, "history.redo", json!({})).await;
    assert_eq!(cells(&s).await[0][0], "Hello");
    tokio::time::timeout(Duration::from_secs(5), async {
        while call(&s, Source::Cli, "agent.runs", json!({})).await["runs"][0]["reverted"] != json!(false) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the run is back");
    call(&s, Source::Cli, "agent.revert", json!({})).await;
    assert_eq!(cells(&s).await[0][0], Value::Null);

    let follow = call(&s, Source::Window, "agent.send", json!({ "prompt": "Thanks", "wait": true })).await;
    assert_eq!(follow["reply"], "You're welcome.");
    let body: Value = serde_json::from_slice(&server.received_requests().await.unwrap()[2].body).unwrap();
    assert_eq!(body["messages"].as_array().unwrap().len(), 5, "the thread goes on");
    assert!(body["tools"].as_array().unwrap().iter().all(|t| !t["name"].as_str().unwrap().starts_with("agent_")), "the agent doesn't drive itself");
    assert_eq!(call(&s, Source::Cli, "agent.runs", json!({})).await["runs"].as_array().unwrap().len(), 2);

    // A new conversation starts empty; the thread is kept on disk with the file until then.
    let saved = std::fs::read_to_string(s.data_dir.join("agent-conversations.json")).unwrap();
    assert!(saved.contains("You're welcome."));
    call(&s, Source::Cli, "agent.newConversation", json!({})).await;
    assert_eq!(call(&s, Source::Cli, "agent.conversation", json!({})).await["entries"], json!([]));
    assert_eq!(call(&s, Source::Cli, "agent.runs", json!({})).await["runs"], json!([]));
}

/// One run at a time, stopped from another client; agents can't choose their own model.
#[tokio::test(flavor = "multi_thread")]
async fn one_run_at_a_time_and_permissions_hold() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(60))).mount(&server).await;
    let (s, _host) = hosted(dir.path(), &server).await;
    let run = call(&s, Source::Window, "agent.send", json!({ "prompt": "Something slow" })).await;
    assert_eq!(run["state"], "running");
    let e = folio_control::call(&s, Source::Cli, "agent.send", json!({ "prompt": "Me too" })).await.unwrap_err();
    assert!(e.contains("still working on run"), "{e}");
    let e = folio_control::call(&s, Source::Cli, "agent.newConversation", json!({})).await.unwrap_err();
    assert!(e.contains("stop it first"), "{e}");
    call(&s, Source::Mcp, "agent.stop", json!({})).await;
    let done = call(&s, Source::Cli, "agent.status", json!({ "wait": true, "timeout": 10 })).await;
    assert_eq!(done["state"], "cancelled", "{done}");

    let e = folio_control::call(&s, Source::Mcp, "agent.setProvider", json!({ "provider": "ollama" })).await.unwrap_err();
    assert!(e.contains("stays with the person"), "{e}");
    // The person picks the model: a provider change clears the last one's model and address.
    let v = call(&s, Source::Cli, "agent.setProvider", json!({ "provider": "codex" })).await;
    assert_eq!((v["provider"].as_str(), v["model"].as_str(), v["baseUrl"].as_str()), (Some("codex"), Some(""), Some("")), "{v}");
    assert!(v["ready"].is_boolean() && v["message"].is_string(), "{v}");
    let e = folio_control::call(&s, Source::Cli, "agent.setProvider", json!({ "provider": "mistrall" })).await.unwrap_err();
    assert!(e.contains("Did you mean mistral?") && e.contains("lsuite, claude-code"), "{e}");
}

/// Another file has its own conversation: a run going on is stopped, and coming back shows the
/// first file's thread again.
#[tokio::test(flavor = "multi_thread")]
async fn each_file_has_its_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let server = mock("/v1/messages", "text/event-stream", vec![anthropic_text("Noted.")]).await;
    let (s, host) = hosted(dir.path(), &server).await;
    call(&s, Source::Window, "agent.send", json!({ "prompt": "Remember this file", "wait": true })).await;
    let first = s.location().unwrap().0;
    s.flush();
    call(&s, Source::Window, "file.new", json!({ "title": "Other" })).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !host.snapshot().entries.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the other file starts afresh");
    assert_eq!(call(&s, Source::Cli, "agent.runs", json!({})).await["runs"], json!([]));
    if first.exists() {
        call(&s, Source::Window, "file.open", json!({ "path": first })).await;
        let entries = host.snapshot().entries;
        assert!(entries.iter().any(|e| matches!(e, Entry::Assistant { text, .. } if text == "Noted.")), "{entries:?}");
        assert!(host.snapshot().runs.iter().all(|r| r.checkpoint.is_none()), "a thread read back can't revert");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn steering_interrupts_a_model_request_and_keeps_the_thread() {
    let dir = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)).set_body_string(anthropic_text("Old direction.")))
        .mount(&server)
        .await;
    let (s, _host) = hosted(dir.path(), &server).await;
    let run = call(&s, Source::Window, "agent.send", json!({ "prompt": "Make the header red" })).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while server.received_requests().await.unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    server.reset().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).insert_header("content-type", "text/event-stream").set_body_string(anthropic_text("Using blue instead.")))
        .mount(&server)
        .await;
    let steer = call(&s, Source::Window, "agent.steer", json!({ "prompt": "Use blue instead" })).await;
    assert_eq!(steer["run"], run["id"]);
    let done = call(&s, Source::Window, "agent.status", json!({ "wait": true, "timeout": 5 })).await;
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["reply"], "Using blue instead.");
    let body: Value = serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert!(body.to_string().contains("Make the header red"));
    assert!(body.to_string().contains("Use blue instead"));
}

#[tokio::test(flavor = "multi_thread")]
async fn unreadable_history_is_never_overwritten() {
    use futures::StreamExt;
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    let mut calls = s.attach_ui();
    tokio::spawn(async move { while calls.next().await.is_some() {} });
    let path = s.data_dir.join("agent-conversations.json");
    std::fs::write(&path, b"{damaged but recoverable history").unwrap();
    let host = Host::install(&s);
    assert!(host.snapshot().storage_error.is_some());
    call(&s, Source::Window, "agent.newConversation", json!({})).await;
    assert_eq!(std::fs::read(&path).unwrap(), b"{damaged but recoverable history");
}

// ---- more providers ---------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn mistral_gets_short_call_ids_through_the_shared_client() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    s.set_secret("mistral", Some("mk-1")).unwrap();
    // Mistral: a whole call without an index.
    let first = openai_sse(&[
        json!({ "choices": [{ "index": 0, "delta": { "role": "assistant", "content": "" } }] }),
        json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "id": "abc123XYZ", "function": { "name": "sheet_set", "arguments": "{\"cell\":\"A1\",\"value\":\"1\"}" } }] }, "finish_reason": "tool_calls" }] }),
    ]);
    let second = openai_sse(&[json!({ "choices": [{ "index": 0, "delta": { "content": "Done." }, "finish_reason": "stop" }] })]);
    let server = mock("/chat/completions", "text/event-stream", vec![first, second]).await;
    let config = AgentConfig { base_url: server.uri(), ..AgentConfig::new(ProviderKind::Mistral) };
    let events = collect(&mut Agent::start(&s, config, "Put 1 in A1", Conversation::new())).await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { changes: 1, .. })), "{events:#?}");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].headers.get("authorization").unwrap(), "Bearer mk-1");
    let body: Value = serde_json::from_slice(&requests[1].body).unwrap();
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs[2]["tool_calls"][0]["id"], "abc123XYZ");
    assert_eq!(msgs[3]["tool_call_id"], "abc123XYZ");
    assert_eq!(body["model"], ProviderKind::Mistral.default_model());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trimmed_tool_list_reaches_every_command_through_folio_run() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    let run_any = openai_sse(&[
        json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "id": "c1", "function": { "name": "folio_run", "arguments": "{\"command\":\"sheet.set\",\"params\":{\"cell\":\"B1\",\"value\":\"2\"}}" } }] } }] }),
        json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }] }),
    ]);
    let done = openai_sse(&[json!({ "choices": [{ "index": 0, "delta": { "content": "Ok." }, "finish_reason": "stop" }] })]);
    let server = mock("/v1/chat/completions", "text/event-stream", vec![run_any, done]).await;
    let config = AgentConfig { base_url: format!("{}/v1", server.uri()), model: "local-model".into(), ..AgentConfig::new(ProviderKind::OpenAiCompatible) };
    let events = collect(&mut Agent::start(&s, config, "Put 2 in B1", Conversation::new())).await;
    assert!(events.iter().any(|e| matches!(e, AgentEvent::Command { record, .. } if record.command == "sheet.set" && record.ok)), "{events:#?}");
    assert_eq!(cells(&s).await[0][1], json!(2.0));
}

#[tokio::test(flavor = "multi_thread")]
async fn gemini_replays_its_parts_with_their_signatures() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    s.set_secret("gemini", Some("AIza-test")).unwrap();
    let call = json!({ "candidates": [{ "content": { "role": "model", "parts": [
        { "text": "Writing.", "thoughtSignature": "sig-1" },
        { "functionCall": { "id": "fc1", "name": "sheet_set", "args": { "cell": "A2", "value": "G" } } },
    ] }, "finishReason": "STOP" }], "usageMetadata": { "promptTokenCount": 10, "candidatesTokenCount": 3 } });
    let done = json!({ "candidates": [{ "content": { "role": "model", "parts": [{ "text": "Done." }] }, "finishReason": "STOP" }] });
    let server = mock("/models/gemini-test:streamGenerateContent", "text/event-stream", vec![format!("data: {call}\n\n"), format!("data: {done}\n\n")]).await;
    let config = AgentConfig { base_url: server.uri(), model: "gemini-test".into(), ..AgentConfig::new(ProviderKind::Gemini) };
    let events = collect(&mut Agent::start(&s, config, "G in A2", Conversation::new())).await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { changes: 1, .. })), "{events:#?}");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[0].headers.get("x-goog-api-key").unwrap(), "AIza-test");
    let first: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(first["tools"][0]["functionDeclarations"].as_array().unwrap().iter().any(|f| f["name"] == "sheet_setRange" && f["parametersJsonSchema"]["type"] == "object"));
    let body: Value = serde_json::from_slice(&requests[1].body).unwrap();
    let contents = body["contents"].as_array().unwrap();
    assert_eq!(contents[1]["role"], "model");
    assert_eq!(contents[1]["parts"][0]["thoughtSignature"], "sig-1");
    assert_eq!(contents[2]["parts"][0]["functionResponse"]["id"], "fc1");
}

// ---- lsuite AI ------------------------------------------------------------------

/// A stand-in lsuite server: `GET /api/account/me` (Pro, opus by default) and the messages API.
async fn lsuite_server(replies: Vec<(u16, String)>) -> MockServer {
    struct Replies(Vec<(u16, String)>, AtomicUsize);
    impl Respond for Replies {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            let i = self.1.fetch_add(1, Ordering::SeqCst).min(self.0.len() - 1);
            let (status, body) = &self.0[i];
            ResponseTemplate::new(*status).insert_header("content-type", if *status == 200 { "text/event-stream" } else { "application/json" }).set_body_string(body.clone())
        }
    }
    let server = MockServer::start().await;
    let me = json!({
        "email": "ada@example.com", "name": "Ada", "plan": "pro", "planName": "Pro", "status": "active", "demo": true,
        "usage": { "used": 1520, "limit": 4000, "percent": 38, "resetsAt": "2026-11-01T00:00:00.000Z" },
        "models": ["claude-sonnet-5-5", "claude-opus-5-5", "claude-haiku-4-5"], "defaultModel": "claude-opus-5-5",
        "manageUrl": format!("{}/account", server.uri()),
    });
    Mock::given(method("GET")).and(path("/api/account/me")).respond_with(ResponseTemplate::new(200).set_body_json(me)).mount(&server).await;
    Mock::given(method("POST")).and(path("/api/ai/v1/messages")).respond_with(Replies(replies, AtomicUsize::new(0))).mount(&server).await;
    server
}

#[tokio::test(flavor = "multi_thread")]
async fn lsuite_ai_streams_a_reply_with_the_account_token() {
    let _lock = account_lock().await;
    let demo = "This is the lsuite AI demo. No model is connected to this server yet, so this answer is canned.";
    let server = lsuite_server(vec![(200, anthropic_text(demo))]).await;
    sign_in(&server.uri(), "lsk_test_token_123");
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    // The settings default: nothing to choose.
    assert_eq!(AgentConfig::from_settings(&s.settings().agent).provider, ProviderKind::Lsuite);
    let status = status_of(&s, ProviderKind::Lsuite).await;
    assert!(status.ready, "{status:?}");
    assert_eq!(status.summary.as_deref(), Some("Pro · 38 % used · resets 1 Nov"));
    assert_eq!(status.default_model, "claude-opus-5-5");

    let mut run = Agent::start(&s, AgentConfig::new(ProviderKind::Lsuite), "Hello", Conversation::new());
    let events = collect(&mut run).await;
    assert!(matches!(events.last(), Some(AgentEvent::Done { summary, .. }) if summary == demo), "{events:#?}");
    let text: String = events.iter().filter_map(|e| if let AgentEvent::Text { delta } = e { Some(delta.as_str()) } else { None }).collect();
    assert_eq!(text, demo);
    let requests: Vec<Request> = server.received_requests().await.unwrap().into_iter().filter(|r| r.url.path() == "/api/ai/v1/messages").collect();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].headers.get("x-api-key").unwrap(), "lsk_test_token_123");
    assert_eq!(requests[0].headers.get("authorization").unwrap(), "Bearer lsk_test_token_123");
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["model"], "claude-opus-5-5", "the plan's default model");
    assert_eq!(body["stream"], true);
    assert!(body.get("cache_control").is_none());
    sign_out();
}

#[tokio::test(flavor = "multi_thread")]
async fn lsuite_ai_errors_are_one_line_with_manage_plan() {
    let _lock = account_lock().await;
    let exhausted = json!({ "type": "error", "error": {
        "type": "allowance_exhausted",
        "message": "Your lsuite AI allowance for this month is used up (Pro, 4,000 credits). It resets on 1 Nov. Manage plan: http://lsuite.test/account",
        "manage_url": "http://lsuite.test/account", "plan": "pro", "resets_at": "2026-11-01T00:00:00.000Z", "used": 4000, "limit": 4000,
    } });
    let server = lsuite_server(vec![(402, exhausted.to_string())]).await;
    sign_in(&server.uri(), "lsk_spent");
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    s.set_secret("anthropic", Some("sk-ant-would-work")).unwrap();
    let mut run = Agent::start(&s, AgentConfig { model: "claude-sonnet-5-5".into(), ..AgentConfig::new(ProviderKind::Lsuite) }, "Hello", Conversation::new());
    let events = collect(&mut run).await;
    let Some(AgentEvent::Error { message, .. }) = events.last() else { panic!("{events:#?}") };
    assert!(message.starts_with("Your lsuite AI allowance for this month is used up"), "{message}");
    assert!(!message.contains('\n'), "one line: {message}");
    assert_eq!(message.matches("Manage plan:").count(), 1, "{message}");
    assert_eq!(lsuite::manage_url(message).as_deref(), Some("http://lsuite.test/account"));
    // Not retried, and never handed to another provider.
    let asked = server.received_requests().await.unwrap().into_iter().filter(|r| r.url.path() == "/api/ai/v1/messages").count();
    assert_eq!(asked, 1);

    // A plan error the same way.
    let server = lsuite_server(vec![(403, json!({ "type": "error", "error": { "type": "model_not_in_plan", "message": "Claude Fable isn't in the Pro plan.", "manage_url": "http://lsuite.test/account" } }).to_string())]).await;
    sign_in(&server.uri(), "lsk_pro");
    let mut run = Agent::start(&s, AgentConfig { model: "claude-fable-5-1".into(), ..AgentConfig::new(ProviderKind::Lsuite) }, "Hello", Conversation::new());
    let events = collect(&mut run).await;
    assert!(matches!(events.last(), Some(AgentEvent::Error { message, .. }) if message == "Claude Fable isn't in the Pro plan. Manage plan: http://lsuite.test/account"), "{events:#?}");
    sign_out();
}

#[tokio::test(flavor = "multi_thread")]
async fn without_an_account_lsuite_ai_asks_to_sign_in() {
    use futures::StreamExt;
    let _lock = account_lock().await;
    sign_out();
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    let mut calls = s.attach_ui();
    tokio::spawn(async move { while calls.next().await.is_some() {} });
    let host = Host::install(&s);
    let status = status_of(&s, ProviderKind::Lsuite).await;
    assert!(!status.ready);
    assert_eq!(status.message, lsuite::SIGN_IN);
    assert_eq!(status.next, Some(Next::Account));
    assert_eq!(status.action.as_ref().and_then(|a| a.folio.as_deref()), Some("account.signIn"));
    let e = folio_control::call(&s, Source::Window, "agent.send", json!({ "prompt": "Hello" })).await.unwrap_err();
    assert_eq!(e, lsuite::SIGN_IN);
    assert!(host.snapshot().runs.is_empty(), "nothing ran, nothing else was tried");
}

// ---- what the person sees ----------------------------------------------------------

#[test]
fn the_context_block_frames_a_request_and_comes_off_again() {
    let g = Glance { lines: vec!["What the person sees:".into(), "Showing sheet \"Data\".".into()], short: "\"Data\"".into() };
    let framed = g.frame("Sum this");
    assert_eq!(framed, "<context>\nWhat the person sees:\nShowing sheet \"Data\".\n</context>\n\nSum this");
    assert_eq!(context::unframed(&framed), "Sum this");
    assert_eq!(context::unframed("no block"), "no block");
    assert_eq!(Glance::default().frame("as is"), "as is");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_is_told_what_the_person_sees() {
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    let page = folio_control::call(&s, Source::Window, "page.list", json!({})).await.unwrap();
    let id = page["pages"][0]["id"].as_str().map(str::to_string).or_else(|| page[0]["id"].as_str().map(str::to_string)).expect("a page id");
    s.set_ui_state(folio_control::UiState { screen: "editor".into(), page: Some(id), cell: Some("B2".into()), range: Some("B2:D9".into()), ..Default::default() });
    let g = glance(&s);
    let block = g.lines.join("\n");
    assert!(block.contains("File \"Test\": 1 page"), "{block}");
    assert!(block.contains("Selected range: B2:D9 (active cell B2)"), "{block}");
    assert!(g.short.ends_with("· B2:D9"), "{}", g.short);
    folio_control::call(&s, Source::Window, "file.close", json!({})).await.unwrap();
    assert_eq!(glance(&s).short, "No file open");
}

/// A real turn against a running lsuite server (its demo answers without an Anthropic key):
/// `FOLIO_TEST_LSUITE_SERVER=http://127.0.0.1:4335 FOLIO_TEST_LSUITE_KEY=lsk_… lcargo test -p folio-agent -- --ignored live_lsuite`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a running lsuite server and a key from its account page"]
async fn live_lsuite_demo_turn() {
    let _lock = account_lock().await;
    let server = std::env::var("FOLIO_TEST_LSUITE_SERVER").expect("FOLIO_TEST_LSUITE_SERVER");
    let key = std::env::var("FOLIO_TEST_LSUITE_KEY").expect("FOLIO_TEST_LSUITE_KEY");
    sign_in(&server, &key);
    let dir = tempfile::tempdir().unwrap();
    let s = with_sheet(dir.path()).await;
    let status = status_of(&s, ProviderKind::Lsuite).await;
    eprintln!("status: {}", serde_json::to_string(&status).unwrap());
    assert!(status.ready, "{status:?}");
    let mut run = Agent::start(&s, AgentConfig::new(ProviderKind::Lsuite), "Say hello.", Conversation::new());
    let events = collect(&mut run).await;
    let reply: String = events.iter().filter_map(|e| if let AgentEvent::Text { delta } = e { Some(delta.as_str()) } else { None }).collect();
    eprintln!("reply: {reply}");
    eprintln!("end: {}", serde_json::to_string(events.last().unwrap()).unwrap());
    assert!(matches!(events.last(), Some(AgentEvent::Done { summary, .. }) if summary == &reply && !reply.is_empty()), "{events:#?}");
    sign_out();
}
