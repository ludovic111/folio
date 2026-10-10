//! The built-in agent's conversation, shared by every client.
//!
//! The app installs one [`Host`] on its session. It owns what the Agent panel shows (the thread,
//! the runs, the requests and replies, one card per command and how each run ended), and the
//! `agent.*` commands drive it, so the panel, `folio-cli` and MCP clients send to, watch, stop
//! and revert the same runs. The panel only draws [`Host::snapshot`], again whenever
//! [`Host::subscribe`] ticks.
//!
//! There is one conversation per file: opening another file stops a run going on, keeps the
//! thread with the file it was about (in `<data>/agent-conversations.json`) and shows that
//! file's own.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use folio_control::registry::Args;
use folio_control::session::{AgentHost, Event};
use folio_control::{CmdResult, CommandRecord, Session, Source};
use futures::StreamExt;
use futures::future::BoxFuture;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::{Agent, AgentConfig, AgentEvent, Conversation, ProviderKind, RunHandle};

/// Entries kept in the conversation; older ones go (indices keep counting).
const MAX_ENTRIES: usize = 2000;

/// Threads kept on disk (the most recently used files).
const MAX_SAVED: usize = 50;

/// How long `wait` waits by default.
const DEFAULT_WAIT: f64 = 900.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Running,
    Done,
    Error,
    Cancelled,
}

/// One request to the agent and what came of it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub id: u64,
    pub prompt: String,
    pub provider: ProviderKind,
    /// The model asked for (empty: the provider's default).
    pub model: String,
    /// Who sent the request: window, cli, mcp or agent.
    pub source: Source,
    pub state: RunState,
    /// What it is doing now, while it runs ("Running sheet.setRange…").
    pub activity: Option<String>,
    /// The agent's reply, as streamed.
    pub reply: String,
    pub error: Option<String>,
    /// Taken before its first change; what agent.revert goes back to.
    pub checkpoint: Option<u64>,
    /// Successful commands that could change the file.
    pub changes: usize,
    /// Every command it ran.
    pub commands: usize,
    pub reverted: bool,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl RunInfo {
    pub fn finished(&self) -> bool {
        self.state != RunState::Running
    }

    pub fn can_revert(&self) -> bool {
        self.finished() && self.checkpoint.is_some() && !self.reverted
    }

    /// Seconds from the request to the end (or to now, while it runs).
    pub fn seconds(&self) -> f64 {
        (self.finished_at.unwrap_or_else(Utc::now) - self.started_at).num_milliseconds().max(0) as f64 / 1000.0
    }

    fn json(&self) -> Value {
        let mut v = json!(self);
        v["canRevert"] = json!(self.can_revert());
        v["seconds"] = json!((self.seconds() * 10.0).round() / 10.0);
        v
    }
}

/// One thing the conversation shows, in the order it happened.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Entry {
    /// A request, from the panel or another client (`source`).
    #[serde(rename_all = "camelCase")]
    User { text: String, run: u64, source: Source },
    /// The agent's reply text.
    Assistant { text: String, run: u64 },
    /// One command, run by the agent (`run`) or by an MCP client or the CLI (`run` null).
    Command { record: Box<CommandRecord>, result: Option<Value>, run: Option<u64> },
    /// How a run ended.
    #[serde(rename_all = "camelCase")]
    Outcome { run: u64, state: RunState, error: Option<String>, changes: usize, seconds: f64, tokens: u64 },
}

/// What the panel draws.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub entries: Vec<Entry>,
    pub runs: Vec<RunInfo>,
    /// The run in progress.
    pub running: Option<u64>,
    /// The file the conversation is about (its id), if one is open.
    pub file: Option<String>,
    /// The conversation history couldn't be read or written.
    pub storage_error: Option<String>,
}

impl Snapshot {
    pub fn run(&self, id: u64) -> Option<&RunInfo> {
        self.runs.iter().find(|r| r.id == id)
    }
}

/// One file's thread on disk. Checkpoints and command sequence numbers belong to the session that
/// made them, so a thread read back can't revert anything.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Saved {
    conversation: Conversation,
    entries: Vec<Entry>,
    runs: Vec<RunInfo>,
    base: usize,
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Archive {
    files: HashMap<String, Saved>,
    next_run: u64,
}

#[derive(Default)]
struct State {
    /// The file the conversation is about.
    file: Option<String>,
    conversation: Conversation,
    entries: Vec<Entry>,
    /// Index of `entries[0]` since the conversation began.
    base: usize,
    /// Command seq → absolute index in the conversation.
    seen: HashMap<u64, usize>,
    runs: Vec<RunInfo>,
    active: Option<(u64, RunHandle)>,
    /// Reply text arrived for the active run.
    streamed: bool,
    next_run: u64,
    /// Runs reverted, the latest last (a redo can bring the latest back).
    reverts: Vec<u64>,
    archive: Archive,
    storage_error: Option<String>,
}

impl State {
    fn run_mut(&mut self, id: u64) -> Option<&mut RunInfo> {
        self.runs.iter_mut().find(|r| r.id == id)
    }

    fn push(&mut self, e: Entry) {
        self.entries.push(e);
        if self.entries.len() > MAX_ENTRIES {
            let drop = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..drop);
            self.base += drop;
            let base = self.base;
            self.seen.retain(|_, i| *i >= base);
        }
    }

    /// Shows a command once, whoever reports it first (the run or the session's stream).
    fn command(&mut self, record: CommandRecord, result: Option<Value>, run: Option<u64>) {
        // `agent.*` calls (a client following a run) aren't part of the conversation.
        if record.command.starts_with("agent.") {
            return;
        }
        let seq = record.seq;
        if seq > 0
            && let Some(&i) = self.seen.get(&seq)
        {
            let base = self.base;
            let mut newly = false;
            if let Some(Entry::Command { result: slot, run: owner, .. }) = self.entries.get_mut(i - base) {
                if result.is_some() {
                    *slot = result;
                }
                if run.is_some() && owner.is_none() {
                    *owner = run;
                    newly = true;
                }
            }
            if newly && let Some(r) = run.and_then(|id| self.run_mut(id)) {
                r.commands += 1;
            }
            return;
        }
        if let Some(r) = run.and_then(|id| self.run_mut(id)) {
            r.commands += 1;
        }
        if seq > 0 {
            self.seen.insert(seq, self.base + self.entries.len());
        }
        self.push(Entry::Command { record: Box::new(record), result, run });
    }

    /// After a redo, the latest reverted run is back when its steps are in the history again.
    fn redone(&mut self, session: &Session) {
        let Some(&id) = self.reverts.last() else { return };
        let Some(cp) = self.runs.iter().find(|r| r.id == id).and_then(|r| r.checkpoint) else { return };
        if session.read(|ed| ed.history().steps_since(cp)).unwrap_or(0) > 0 {
            self.reverts.pop();
            if let Some(r) = self.run_mut(id) {
                r.reverted = false;
            }
        }
    }

    fn save_current(&mut self) {
        let Some(key) = self.file.clone() else { return };
        if self.entries.is_empty() && self.runs.is_empty() {
            self.archive.files.remove(&key);
        } else {
            let saved = Saved { conversation: self.conversation.clone(), entries: self.entries.clone(), runs: self.runs.clone(), base: self.base, updated_at: Some(Utc::now()) };
            self.archive.files.insert(key, saved);
        }
        if self.archive.files.len() > MAX_SAVED {
            let mut keys: Vec<(String, Option<DateTime<Utc>>)> = self.archive.files.iter().map(|(k, v)| (k.clone(), v.updated_at)).collect();
            keys.sort_by_key(|(_, at)| *at);
            for (k, _) in keys.into_iter().take(self.archive.files.len() - MAX_SAVED) {
                self.archive.files.remove(&k);
            }
        }
        self.archive.next_run = self.next_run;
    }

    fn load_file(&mut self) {
        let saved = self.file.as_ref().and_then(|k| self.archive.files.get(k)).cloned().unwrap_or_default();
        self.conversation = saved.conversation;
        self.entries = saved.entries;
        self.runs = saved.runs;
        self.base = saved.base;
        self.seen.clear();
        self.reverts.clear();
        // Checkpoints belong to the session that took them.
        for r in &mut self.runs {
            r.checkpoint = None;
        }
    }

    fn clear_thread(&mut self) {
        self.base += self.entries.len();
        self.entries.clear();
        self.seen.clear();
        self.runs.clear();
        self.reverts.clear();
        self.conversation = Conversation::new();
    }
}

pub struct Host {
    state: Mutex<State>,
    changed: watch::Sender<u64>,
    storage_path: PathBuf,
    /// The file on disk couldn't be read: it is never overwritten.
    storage_read_error: bool,
}

impl Host {
    /// Makes the host, listens to the session and answers its `agent.*` commands. Call once,
    /// inside the session's runtime or not.
    pub fn install(session: &Arc<Session>) -> Arc<Self> {
        let (changed, _) = watch::channel(0);
        let storage_path = session.data_dir.join("agent-conversations.json");
        let (archive, storage_error) = match read_archive(&storage_path) {
            Ok(a) => (a, None),
            Err(e) => (Archive::default(), Some(e)),
        };
        let mut state = State { file: session.current_id().map(|i| i.to_string()), next_run: archive.next_run.max(1), archive, storage_error: storage_error.clone(), ..Default::default() };
        state.load_file();
        let host = Arc::new(Self { state: Mutex::new(state), changed, storage_path, storage_read_error: storage_error.is_some() });
        let mut events = session.subscribe();
        let (weak, weak_session) = (Arc::downgrade(&host), Arc::downgrade(session));
        session.runtime().spawn(async move {
            loop {
                let event = match events.recv().await {
                    Ok(e) => e,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                };
                let (Some(host), Some(session)) = (weak.upgrade(), weak_session.upgrade()) else { break };
                host.on_session_event(&session, event);
            }
        });
        session.set_agent_host(host.clone());
        host
    }

    /// Ticks after every change of what [`snapshot`](Self::snapshot) returns.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    pub fn snapshot(&self) -> Snapshot {
        let st = self.state.lock();
        Snapshot {
            entries: st.entries.clone(),
            runs: st.runs.clone(),
            running: st.active.as_ref().map(|(id, _)| *id),
            file: st.file.clone(),
            storage_error: st.storage_error.clone(),
        }
    }

    /// The run in progress, if any.
    pub fn running(&self) -> Option<u64> {
        self.state.lock().active.as_ref().map(|(id, _)| *id)
    }

    fn notify(&self) {
        self.changed.send_modify(|v| *v += 1);
    }

    fn persist(&self) {
        let mut st = self.state.lock();
        st.save_current();
        if self.storage_read_error {
            return;
        }
        match write_archive(&self.storage_path, &st.archive) {
            Ok(()) => st.storage_error = None,
            Err(e) => {
                tracing::warn!("the agent's conversations couldn't be saved: {e}");
                st.storage_error = Some(e);
            }
        }
    }

    fn on_session_event(&self, session: &Session, event: Event) {
        match event {
            Event::Command { record } => {
                let mut st = self.state.lock();
                if record.ok && record.command == "history.redo" {
                    st.redone(session);
                }
                // Commands of MCP clients and the CLI show as cards; the window's own don't.
                if record.source != Source::Window {
                    st.command(record, None, None);
                }
                drop(st);
                self.notify();
            }
            Event::DocSwitched { .. } => self.follow_file(session),
            _ => {}
        }
    }

    /// Another file is open: a run going on stops, and the conversation is that file's.
    fn follow_file(&self, session: &Session) {
        // Events arrive late; the live file is what counts.
        let file = session.current_id().map(|i| i.to_string());
        let mut st = self.state.lock();
        if st.file == file {
            return;
        }
        if let Some((id, handle)) = st.active.take() {
            handle.cancel();
            st.conversation = handle.conversation();
            finish(&mut st, id, RunState::Cancelled, None, None, handle.changes());
        }
        st.save_current();
        st.file = file;
        st.load_file();
        drop(st);
        self.persist();
        self.notify();
    }

    // ---- runs -----------------------------------------------------------------

    /// Starts a run with the provider in `settings.agent`, continuing the conversation.
    pub fn send(self: &Arc<Self>, session: &Arc<Session>, source: Source, prompt: &str) -> CmdResult<RunInfo> {
        self.follow_file(session);
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() {
            return Err("Write a request for the agent first.".into());
        }
        let config = AgentConfig::from_settings(&session.settings().agent);
        let mut st = self.state.lock();
        if let Some((id, _)) = &st.active {
            let what = st.runs.iter().find(|r| r.id == *id).map(|r| format!(" (\"{}\")", crate::tools::bounded(&r.prompt, 80))).unwrap_or_default();
            return Err(format!("The agent is still working on run {id}{what}. Wait for it (agent.status wait=true), steer it (agent.steer) or stop it (agent.stop)."));
        }
        let id = st.next_run;
        st.next_run += 1;
        let info = RunInfo {
            id,
            prompt: prompt.clone(),
            provider: config.provider,
            model: config.model.clone(),
            source,
            state: RunState::Running,
            activity: Some(format!("Starting {}…", config.provider.label())),
            reply: String::new(),
            error: None,
            checkpoint: None,
            changes: 0,
            commands: 0,
            reverted: false,
            started_at: Utc::now(),
            finished_at: None,
            input_tokens: 0,
            output_tokens: 0,
        };
        st.runs.push(info.clone());
        st.push(Entry::User { text: prompt.clone(), run: id, source });
        st.streamed = false;
        // The run lives on the session's runtime; this task only reads its events.
        let mut run = Agent::start(session, config, prompt, st.conversation.clone());
        st.active = Some((id, run.handle()));
        drop(st);
        self.persist();
        self.notify();
        let Some(mut events) = run.take_events() else { return Ok(info) };
        let host = self.clone();
        session.runtime().spawn(async move {
            while let Some(ev) = events.next().await {
                host.on_run_event(id, ev);
            }
        });
        Ok(info)
    }

    fn on_run_event(&self, id: u64, ev: AgentEvent) {
        let terminal = ev.is_terminal();
        let mut st = self.state.lock();
        // A run stopped by a file switch still reports its end: it belongs to no conversation.
        let current = st.active.as_ref().is_some_and(|(a, _)| *a == id);
        match ev {
            AgentEvent::Status { message } => {
                if let Some(r) = st.run_mut(id).filter(|r| !r.finished()) {
                    r.activity = Some(message);
                }
            }
            AgentEvent::Text { delta } if current => {
                st.streamed = true;
                if let Some(r) = st.run_mut(id) {
                    r.reply.push_str(&delta);
                }
                match st.entries.last_mut() {
                    Some(Entry::Assistant { text, run }) if *run == id => text.push_str(&delta),
                    _ => st.push(Entry::Assistant { text: delta.trim_start().to_string(), run: id }),
                }
            }
            AgentEvent::Command { record, result } if current => st.command(*record, result, Some(id)),
            AgentEvent::Usage { input_tokens, output_tokens } => {
                if let Some(r) = st.run_mut(id) {
                    r.input_tokens += input_tokens;
                    r.output_tokens += output_tokens;
                }
            }
            AgentEvent::Done { summary, checkpoint, changes, conversation } if current => {
                st.conversation = conversation;
                if !st.streamed && !summary.trim().is_empty() {
                    if let Some(r) = st.run_mut(id) {
                        r.reply = summary.clone();
                    }
                    st.push(Entry::Assistant { text: summary, run: id });
                }
                finish(&mut st, id, RunState::Done, None, checkpoint, changes);
            }
            AgentEvent::Error { message, checkpoint, changes } if current => {
                if let Some((_, h)) = &st.active {
                    st.conversation = h.conversation();
                }
                finish(&mut st, id, RunState::Error, Some(message), checkpoint, changes);
            }
            AgentEvent::Cancelled { checkpoint, changes } if current => {
                if let Some((_, h)) = &st.active {
                    st.conversation = h.conversation();
                }
                finish(&mut st, id, RunState::Cancelled, None, checkpoint, changes);
            }
            _ => return,
        }
        drop(st);
        if terminal {
            self.persist();
        }
        self.notify();
    }

    /// Stops the run in progress; its finished edits stay.
    pub fn stop(&self) -> CmdResult<u64> {
        let mut st = self.state.lock();
        let (id, handle) = st.active.clone().ok_or("The agent isn't working on anything.")?;
        handle.cancel();
        if let Some(r) = st.run_mut(id) {
            r.activity = Some("Stopping…".into());
        }
        drop(st);
        self.notify();
        Ok(id)
    }

    /// Redirects the run in progress with a follow-up message.
    pub fn steer(&self, source: Source, prompt: &str) -> CmdResult<u64> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("Write a follow-up message first.".into());
        }
        let mut st = self.state.lock();
        let (id, handle) = st.active.clone().ok_or("The agent isn't working on anything: send a new message instead (agent.send).")?;
        handle.steer(prompt.to_string())?;
        st.push(Entry::User { text: prompt.to_string(), run: id, source });
        drop(st);
        self.notify();
        Ok(id)
    }

    /// Puts the file back as it was before a run (the latest revertible one by default).
    pub async fn revert(&self, session: &Arc<Session>, source: Source, run: Option<u64>) -> CmdResult {
        let (id, checkpoint) = {
            let st = self.state.lock();
            let r = match run {
                Some(id) => st.runs.iter().find(|r| r.id == id).ok_or_else(|| format!("No run {id} in this conversation. agent.runs lists them."))?,
                None => st.runs.iter().rev().find(|r| r.can_revert()).ok_or("No agent run to revert: none changed the file, or they are reverted already.")?,
            };
            if !r.finished() {
                return Err(format!("Run {} is still going: stop it first (agent.stop).", r.id));
            }
            if r.reverted {
                return Err(format!("Run {} is reverted already (history.redo brings it back).", r.id));
            }
            (r.id, r.checkpoint.ok_or_else(|| format!("Run {} changed nothing in this session: there is nothing to revert.", r.id))?)
        };
        let result = folio_control::call(session, source, "history.revertTo", json!({ "checkpoint": checkpoint })).await?;
        let mut st = self.state.lock();
        if let Some(r) = st.run_mut(id) {
            r.reverted = true;
        }
        st.reverts.push(id);
        drop(st);
        self.persist();
        self.notify();
        Ok(json!({ "run": id, "checkpoint": checkpoint, "result": result }))
    }

    /// Starts a new conversation about the same file (the last one is gone).
    pub fn new_conversation(&self) -> CmdResult<()> {
        let mut st = self.state.lock();
        if let Some((id, _)) = &st.active {
            return Err(format!("The agent is working on run {id}: stop it first (agent.stop)."));
        }
        st.clear_thread();
        drop(st);
        self.persist();
        self.notify();
        Ok(())
    }

    /// The run once it has ended, or as it is when `timeout` passes.
    pub async fn wait(&self, id: u64, timeout: Duration) -> CmdResult<(RunInfo, bool)> {
        let mut rx = self.subscribe();
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let run = self.state.lock().runs.iter().find(|r| r.id == id).cloned().ok_or_else(|| format!("Run {id} is gone: another file was opened."))?;
            if run.finished() {
                return Ok((run, false));
            }
            match tokio::time::timeout_at(deadline, rx.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) | Err(_) => return Ok((run, true)),
            }
        }
    }

    /// A run with the commands it ran, for `agent.status` and `agent.send wait=true`.
    fn run_report(&self, run: &RunInfo, timed_out: bool) -> Value {
        let st = self.state.lock();
        let commands: Vec<Value> = st
            .entries
            .iter()
            .filter_map(|e| match e {
                Entry::Command { record, result, run: Some(r) } if *r == run.id => Some(json!({
                    "seq": record.seq,
                    "command": record.command,
                    "params": record.params,
                    "ok": record.ok,
                    "error": record.error,
                    "result": result.as_ref().or(record.result.as_ref()).filter(|v| v.to_string().len() <= 4096),
                })),
                _ => None,
            })
            .collect();
        let mut v = run.json();
        v["commandList"] = json!(commands);
        if timed_out {
            v["waitTimedOut"] = json!(true);
        }
        v
    }

    fn pick(&self, run: Option<u64>) -> CmdResult<RunInfo> {
        let st = self.state.lock();
        match run {
            Some(id) => st.runs.iter().find(|r| r.id == id).cloned().ok_or_else(|| format!("No run {id} in this conversation. agent.runs lists them.")),
            None => st.runs.last().cloned().ok_or_else(|| "The agent hasn't been asked anything yet about this file. agent.send asks it.".into()),
        }
    }

    async fn command(self: Arc<Self>, session: Arc<Session>, source: Source, command: &'static str, a: Args) -> CmdResult {
        self.follow_file(&session);
        let timeout = Duration::from_secs_f64(a.opt_f64("timeout").unwrap_or(DEFAULT_WAIT).clamp(0.1, 86_400.0));
        match command {
            "agent.providers" => {
                let config = AgentConfig::from_settings(&session.settings().agent);
                let statuses = crate::provider_status(&session).await;
                // Every provider with its models, for a model picker: the chosen one's fetched
                // (and kept a few hours), the others' as last fetched or built in.
                let active_models = crate::list_models(&session, config.provider, a.bool_or("refresh", false)).await;
                let list: Vec<Value> = statuses
                    .iter()
                    .map(|p| {
                        let (models, source) = if p.provider == config.provider { (active_models.models.clone(), active_models.source) } else { crate::models::known(p.provider) };
                        let mut v = json!(p);
                        v["modelList"] = json!(models);
                        v["modelSource"] = json!(source);
                        v
                    })
                    .collect();
                let groups: Vec<Value> = crate::Group::ALL.iter().map(|g| json!({ "id": g, "label": g.label() })).collect();
                let model = statuses.iter().find(|p| p.active).map(|p| if config.model.is_empty() { p.default_model.clone() } else { config.model.clone() }).unwrap_or_default();
                Ok(json!({
                    "provider": config.provider,
                    "model": model,
                    "baseUrl": config.base_url(),
                    "enabled": session.settings().agent.enabled,
                    "groups": groups,
                    "providers": list,
                }))
            }
            "agent.models" => {
                let kind = match a.opt_str("provider") {
                    Some(id) => parse_provider(id)?,
                    None => AgentConfig::from_settings(&session.settings().agent).provider,
                };
                Ok(json!(crate::list_models(&session, kind, a.bool_or("refresh", false)).await))
            }
            "agent.setProvider" => {
                let kind = parse_provider(a.str("provider")?)?;
                let (model, base) = (a.opt_str("model").map(str::trim).map(str::to_string), a.opt_str("baseUrl").map(str::trim).map(str::to_string));
                let s = session.update_settings(|st| {
                    let changed = st.agent.provider != kind.id();
                    st.agent.provider = kind.id().to_string();
                    // A model or address belongs to the provider it was chosen for.
                    if let Some(m) = model.clone().or_else(|| changed.then(String::new)) {
                        st.agent.model = m;
                    }
                    if let Some(b) = base.clone().or_else(|| changed.then(String::new)) {
                        st.agent.base_url = b;
                    }
                })?;
                let status = crate::status_of(&session, kind).await;
                Ok(json!({ "provider": s.agent.provider, "model": s.agent.model, "baseUrl": s.agent.base_url, "ready": status.ready, "message": status.message, "next": status.next, "action": status.action }))
            }
            "agent.send" => {
                let info = self.send(&session, source, a.str("prompt")?)?;
                if !a.bool_or("wait", false) {
                    return Ok(self.run_report(&info, false));
                }
                let (run, timed_out) = self.wait(info.id, timeout).await?;
                Ok(self.run_report(&run, timed_out))
            }
            "agent.steer" => Ok(json!({ "run": self.steer(source, a.str("prompt")?)?, "accepted": true })),
            "agent.status" => {
                let mut run = self.pick(a.opt_i64("run").map(|v| v.max(0) as u64))?;
                let mut timed_out = false;
                if a.bool_or("wait", false) {
                    (run, timed_out) = self.wait(run.id, timeout).await?;
                }
                let mut v = self.run_report(&run, timed_out);
                v["running"] = json!(self.running());
                Ok(v)
            }
            "agent.runs" => {
                let st = self.state.lock();
                Ok(json!({ "runs": st.runs.iter().map(RunInfo::json).collect::<Vec<_>>(), "running": st.active.as_ref().map(|(id, _)| *id) }))
            }
            "agent.conversation" => {
                let st = self.state.lock();
                let since = a.opt_i64("since").map(|v| v.max(0) as usize).unwrap_or(st.base).max(st.base);
                let entries: Vec<&Entry> = st.entries.iter().skip(since - st.base).collect();
                Ok(json!({ "from": since, "next": st.base + st.entries.len(), "entries": entries, "running": st.active.as_ref().map(|(id, _)| *id) }))
            }
            "agent.stop" => Ok(json!({ "stopping": self.stop()? })),
            "agent.revert" => self.revert(&session, source, a.opt_i64("run").map(|v| v.max(0) as u64)).await,
            "agent.newConversation" => {
                self.new_conversation()?;
                Ok(json!({ "entries": 0 }))
            }
            other => Err(format!("`{other}` is not implemented")),
        }
    }
}

/// A provider id, or what to type instead.
fn parse_provider(id: &str) -> CmdResult<ProviderKind> {
    ProviderKind::parse(id).ok_or_else(|| {
        let ids: Vec<&str> = ProviderKind::ALL.iter().map(|k| k.id()).collect();
        let hint = folio_control::registry::closest(id, &ids).map(|c| format!(" Did you mean {c}?")).unwrap_or_default();
        format!("Unknown agent provider `{id}`.{hint} Providers: {}.", ids.join(", "))
    })
}

fn finish(st: &mut State, id: u64, state: RunState, error: Option<String>, checkpoint: Option<u64>, changes: usize) {
    st.active = None;
    let Some(r) = st.run_mut(id) else { return };
    r.state = state;
    r.activity = None;
    r.error = error.clone();
    r.checkpoint = checkpoint;
    r.changes = changes;
    r.finished_at = Some(Utc::now());
    let (seconds, tokens) = (r.seconds(), r.input_tokens + r.output_tokens);
    st.push(Entry::Outcome { run: id, state, error, changes, seconds, tokens });
}

fn read_archive(path: &Path) -> CmdResult<Archive> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Archive::default()),
        Err(e) => return Err(format!("Couldn't read the agent's conversations at {}: {e}. They aren't saved until it can be read.", path.display())),
    };
    let mut archive: Archive = serde_json::from_slice(&bytes).map_err(|e| {
        format!("The agent's conversations at {} can't be read ({e}). They aren't saved, so the file stays as it is.", path.display())
    })?;
    for saved in archive.files.values_mut() {
        for r in &mut saved.runs {
            r.checkpoint = None;
            if r.state == RunState::Running {
                r.state = RunState::Cancelled;
                r.activity = None;
                r.finished_at = Some(Utc::now());
            }
        }
    }
    Ok(archive)
}

fn write_archive(path: &Path, archive: &Archive) -> CmdResult<()> {
    let parent = path.parent().ok_or("no folder for the agent's conversations")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, archive).map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| format!("Couldn't save the agent's conversations: {e}"))?;
    Ok(())
}

impl AgentHost for Host {
    fn call(self: Arc<Self>, session: Arc<Session>, source: Source, command: &'static str, args: Args) -> BoxFuture<'static, CmdResult> {
        Box::pin(self.command(session, source, command, args))
    }
}
