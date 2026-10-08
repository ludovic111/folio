//! The Agent panel, docked on the right (⌘J). lsuite AI comes first: sign in once and the agent
//! works, nothing to install. The person's own Claude Code or Codex, an API key or a local model
//! work too. Whatever runs it, the agent acts only through the command registry
//! (`folio_agent::Host`), so permissions and the one undo history are the same as for MCP and the
//! CLI. The panel shows one card per command (the agent's own, and those of MCP clients and the
//! CLI driving folio), how each run ended with "Revert this run", and the composer.
//!
//! The conversation is the host's: the panel sends, steers, stops and reverts with the `agent.*`
//! commands, as `folio-cli` and MCP clients can, and draws the host's snapshot.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use folio_agent::{AgentConfig, Entry, Host, ProviderKind, ProviderStatus, RunInfo, RunState, Snapshot};
use folio_control::{CommandRecord, Source};
use gpui::{AnimationExt as _, AnyElement, App, Context, Div, Entity, FontWeight, Hsla, Render, ScrollHandle, SharedString, Subscription, Task, Window, div, prelude::*, px};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::store::{Dialog, MenuEntry, MenuItem, Store, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::{Button, GlassExt, caps, icon, logo, panel_title, segmented};

const EXAMPLES: [&str; 4] = [
    "Summarise this file in five bullet points at the top of the document",
    "Add a sheet of monthly costs with totals and a chart",
    "Make a five-slide deck from the document, with speaker notes",
    "Put a live table of the budget sheet after the introduction",
];

/// Most characters of a command's answer shown in an open card.
const RESULT_PREVIEW: usize = 1600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Conversation,
    Changes,
}

/// One undo step as `history.list` reports it.
#[derive(Clone, Debug, Deserialize)]
struct Step {
    label: String,
    source: String,
}

/// `history.list`: undo steps newest first, redo steps next first.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct History {
    #[serde(default)]
    undo: Vec<Step>,
    #[serde(default)]
    redo: Vec<Step>,
    #[serde(default)]
    can_undo: bool,
    #[serde(default)]
    can_redo: bool,
}

pub struct AgentPanel {
    store: Entity<Store>,
    tab: Tab,
    composer: Entity<TextInput>,
    /// The host's conversation and runs, as last drawn.
    snap: Snapshot,
    statuses: Vec<ProviderStatus>,
    checking: bool,
    history: Option<History>,
    /// Terminal sessions (by checkpoint) reverted from the Changes tab.
    reverted_sessions: HashSet<u64>,
    expanded: HashSet<u64>,
    scroll: ScrollHandle,
    changes_scroll: ScrollHandle,
    was_open: bool,
    provider_seen: String,
    signed_in_seen: bool,
    _pump: Option<Task<()>>,
    _ticker: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl AgentPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let composer = cx.new(|cx| {
            let mut i = TextInput::new(cx).multiline(2).placeholder("Ask the agent to write, calculate or build slides…");
            i.bare = true;
            i.submit_on_enter = true;
            i
        });
        let subs = vec![
            cx.observe(&store, |this, _, cx| this.on_store_changed(cx)),
            cx.subscribe(&composer, |this, _, e: &InputEvent, cx| match e {
                InputEvent::Submit => this.send(cx),
                InputEvent::Changed(_) => cx.notify(),
                _ => {}
            }),
        ];
        // Redraw whenever the host's conversation changes, whoever changed it.
        let host = store.read(cx).agent.clone();
        let pump = host.as_ref().map(|host| {
            let mut changes = host.subscribe();
            cx.spawn(async move |this, cx| {
                while changes.changed().await.is_ok() {
                    if this.update(cx, |p, cx| p.refresh(cx)).is_err() {
                        break;
                    }
                }
            })
        });
        let provider_seen = store.read(cx).settings.agent.provider.clone();
        let mut this = Self {
            store,
            tab: Tab::Conversation,
            composer,
            snap: host.as_ref().map(|h| h.snapshot()).unwrap_or_default(),
            statuses: vec![],
            checking: false,
            history: None,
            reverted_sessions: HashSet::new(),
            expanded: HashSet::new(),
            scroll: ScrollHandle::new(),
            changes_scroll: ScrollHandle::new(),
            was_open: false,
            provider_seen,
            signed_in_seen: false,
            _pump: pump,
            _ticker: None,
            _subs: subs,
        };
        this.on_store_changed(cx);
        this
    }

    fn host(&self, cx: &App) -> Option<Arc<Host>> {
        self.store.read(cx).agent.clone()
    }

    fn provider(&self, cx: &App) -> ProviderKind {
        AgentConfig::from_settings(&self.store.read(cx).settings.agent).provider
    }

    fn status_of(&self, kind: ProviderKind) -> Option<&ProviderStatus> {
        self.statuses.iter().find(|s| s.provider == kind)
    }

    // ---- store and session -------------------------------------------------

    fn on_store_changed(&mut self, cx: &mut Context<Self>) {
        let s = self.store.read(cx);
        let open = s.agent_open;
        let provider = s.settings.agent.provider.clone();
        let signed_in = s.account["signedIn"] == true;
        let recheck = !self.was_open || provider != self.provider_seen || signed_in != self.signed_in_seen;
        if open && recheck {
            self.refresh_statuses(cx);
            self.refresh_history(cx);
        }
        if open && !self.was_open {
            // The plan and the allowance change on the server.
            self.store.update(cx, |s, cx| s.refresh_account(cx));
        }
        self.was_open = open;
        self.provider_seen = provider;
        self.signed_in_seen = signed_in;
        cx.notify();
    }

    fn refresh_statuses(&mut self, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        self.checking = true;
        let session = self.store.read(cx).session.clone();
        // Probes the CLIs, the local servers and lsuite: never on the UI thread.
        let task = gpui_tokio::Tokio::spawn(cx, async move { folio_agent::provider_status(&session).await });
        cx.spawn(async move |this, cx| {
            let r = task.await;
            this.update(cx, |p, cx| {
                p.checking = false;
                if let Ok(list) = r {
                    p.statuses = list;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn refresh_history(&mut self, cx: &mut Context<Self>) {
        let session = self.store.read(cx).session.clone();
        if !session.is_open() {
            self.history = None;
            return;
        }
        let task = gpui_tokio::Tokio::spawn(cx, async move { folio_control::call(&session, Source::Window, "history.list", json!({})).await });
        cx.spawn(async move |this, cx| {
            let r = task.await;
            this.update(cx, |p, cx| {
                p.history = r.ok().and_then(Result::ok).and_then(|v| serde_json::from_value(v).ok());
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Takes the host's latest snapshot.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(host) = self.host(cx) else { return };
        let snap = host.snapshot();
        let grew = snap.entries.len() != self.snap.entries.len() || snap.entries.last().map(entry_len) != self.snap.entries.last().map(entry_len);
        let ended = self.snap.running.is_some() && snap.running.is_none();
        let started = snap.running.is_some() && self._ticker.is_none();
        if snap.entries.is_empty() {
            self.expanded.clear();
        }
        self.snap = snap;
        if grew {
            self.scroll.scroll_to_bottom();
        }
        if ended {
            self._ticker = None;
            self.refresh_history(cx);
            // A run spends allowance: the summary changes.
            if self.provider(cx) == ProviderKind::Lsuite {
                self.store.update(cx, |s, cx| s.refresh_account(cx));
            }
        }
        if started {
            // Keeps the elapsed time moving while the model thinks.
            self._ticker = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    let going = this.update(cx, |p, cx| {
                        cx.notify();
                        p.snap.running.is_some()
                    });
                    if !matches!(going, Ok(true)) {
                        break;
                    }
                }
            }));
        }
        cx.notify();
    }

    // ---- actions ---------------------------------------------------------------

    fn send(&mut self, cx: &mut Context<Self>) {
        let prompt = self.composer.read(cx).text().trim().to_string();
        if prompt.is_empty() {
            return;
        }
        self.composer.update(cx, |i, cx| i.set_text("", cx));
        let command = if self.snap.running.is_some() { "agent.steer" } else { "agent.send" };
        self.store.update(cx, |s, cx| s.run(command, json!({ "prompt": prompt }), cx));
        self.tab = Tab::Conversation;
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.run("agent.stop", json!({}), cx));
    }

    /// "Revert this run": back to the checkpoint taken before its first change.
    fn revert_run(&mut self, run: u64, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        self.store.update(cx, |s, cx| {
            s.run_then("agent.revert", json!({ "run": run }), cx, move |s, _, cx| {
                s.flash("Reverted the run. Redo brings it back.", cx);
                this.update(cx, |p, cx| p.refresh_history(cx)).ok();
            })
        });
    }

    fn revert_session(&mut self, checkpoint: u64, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        self.store.update(cx, |s, cx| {
            s.run_then("history.revertTo", json!({ "checkpoint": checkpoint }), cx, move |s, _, cx| {
                s.flash("Reverted the session. Redo brings it back.", cx);
                this.update(cx, |p, cx| p.refresh_history(cx)).ok();
            })
        });
        self.reverted_sessions.insert(checkpoint);
        cx.notify();
    }

    fn new_conversation(&mut self, cx: &mut Context<Self>) {
        if self.snap.running.is_none() {
            self.store.update(cx, |s, cx| s.run("agent.newConversation", json!({}), cx));
        }
    }

    fn open_agent_settings(cx: &mut App) {
        cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Settings { section: Some("agent".into()) }, cx));
    }

    fn sign_in(cx: &mut App) {
        cx.store().update(cx, |s, cx| s.run_then("account.signIn", json!({}), cx, |s, _, cx| s.flash("Finish signing in in your browser.", cx)));
    }

    fn provider_menu(&mut self, position: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let current = self.provider(cx);
        let this = cx.entity().downgrade();
        let mut entries: Vec<MenuEntry> = vec![];
        for group in folio_agent::Group::ALL {
            if !entries.is_empty() {
                entries.push(MenuEntry::Separator);
            }
            entries.extend(ProviderKind::ALL.into_iter().filter(|k| k.group() == group).map(|kind| {
                let ready = self.status_of(kind).map(|s| s.ready);
                let mut item = MenuItem::new(kind.label(), move |_, cx| {
                    cx.store().update(cx, |s, cx| s.run("agent.setProvider", json!({ "provider": kind.id() }), cx));
                })
                .shortcut(match (kind, ready) {
                    (ProviderKind::Lsuite, Some(false)) => "sign in",
                    (_, Some(true)) => "ready",
                    (_, Some(false)) => "set up",
                    (_, None) => "…",
                });
                if kind == current {
                    item = item.icon("check");
                }
                item.entry()
            }));
        }
        entries.push(MenuEntry::Separator);
        entries.push(
            MenuItem::new("Check again", move |_, cx| {
                this.update(cx, |p, cx| p.refresh_statuses(cx)).ok();
            })
            .icon("refresh-cw")
            .entry(),
        );
        entries.push(MenuItem::new("Agent settings and permissions…", |_, cx| Self::open_agent_settings(cx)).icon("shield-check").entry());
        self.store.update(cx, |s, cx| s.open_menu(position, entries, cx));
    }

    // ---- rendering -------------------------------------------------------------

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let kind = self.provider(cx);
        let ready = self.status_of(kind).map(|s| s.ready);
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(44.))
            .px(px(12.))
            .border_b_1()
            .border_color(t.line)
            .child(panel_title("Agent"))
            .child(
                div()
                    .id("agent-provider")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .min_w_0()
                    .h(px(26.))
                    .px(px(8.))
                    .border_1()
                    .border_color(t.line_strong)
                    .text_size(px(sz::SM))
                    .text_color(t.text)
                    .cursor_pointer()
                    .hover(|s| s.bg(t.hover))
                    .tooltip(|_, cx| crate::ui::tooltip("What runs the agent".into(), cx))
                    .on_click(cx.listener(|this, e: &gpui::ClickEvent, _, cx| this.provider_menu(e.position(), cx)))
                    .child(logo(kind.id(), px(14.)))
                    .child(div().truncate().child(kind.label()))
                    // Ready: a filled square; to set up: an empty one; checking: none.
                    .when_some(ready, |d, ok| d.child(div().flex_none().size(px(6.)).border_1().border_color(t.text_2).when(ok, |d| d.bg(t.text_2))))
                    .child(icon("chevron-down").size(px(12.))),
            )
            .child(div().flex_1())
            .child(
                crate::ui::group(
                    [
                        Button::icon("agent-new", "plus", "New conversation").small().flush().disabled(self.snap.running.is_some()).on_click(cx.listener(|this, _, _, cx| this.new_conversation(cx))).into_any_element(),
                        Button::icon("agent-settings", "shield-check", "Agent settings and permissions").small().flush().on_click(|_, _, cx| Self::open_agent_settings(cx)).into_any_element(),
                        Button::icon("agent-close", "x", crate::actions::tip("Close", &crate::actions::ToggleAgent))
                            .small()
                            .flush()
                            .on_click(|_, w, cx| w.dispatch_action(Box::new(crate::actions::ToggleAgent), cx))
                            .into_any_element(),
                    ],
                    cx,
                ),
            )
    }

    /// lsuite AI's account strip: the plan and the allowance with Manage plan and Sign out, or
    /// Sign in.
    fn lsuite_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let acc = self.store.read(cx).account.clone();
        let signed = acc["signedIn"] == true;
        let manage = acc["manageUrl"].as_str().unwrap_or("https://lsuite.xyz/account").to_string();
        let status = self.status_of(ProviderKind::Lsuite);
        let free = signed && acc["plan"].as_str().is_none_or(|p| p == "free" || p.is_empty()) && acc["offline"] != true;
        let line = if signed {
            acc["summary"].as_str().or(acc["plan"].as_str()).filter(|s| !s.is_empty()).unwrap_or("Signed in").to_string()
        } else {
            "No setup. Sign in and your agent works.".to_string()
        };
        div()
            .flex()
            .flex_none()
            .flex_col()
            .gap(px(8.))
            .p(px(12.))
            .border_b_1()
            .border_color(t.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(logo("lsuite", px(26.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(div().truncate().text_size(px(sz::BASE)).font_weight(FontWeight::SEMIBOLD).child(if signed { acc["email"].as_str().unwrap_or("lsuite AI").to_string() } else { "lsuite AI".into() }))
                            .child(div().truncate().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_2).child(if signed { line.to_uppercase() } else { line })),
                    ),
            )
            .when(free, |d| d.child(div().text_size(px(sz::SM)).text_color(t.text_2).child("Your account is on Free: lsuite AI needs a plan. Bringing your own provider stays free.")))
            .when(acc["offline"] == true, |d| d.child(div().text_size(px(sz::SM)).text_color(t.text_2).child(format!("lsuite didn't answer: {}", acc["error"].as_str().unwrap_or("")))))
            .when(signed && status.is_some_and(|s| !s.ready) && !free, |d| d.child(div().text_size(px(sz::SM)).text_color(t.text_2).child(status.map(|s| s.message.clone()).unwrap_or_default())))
            .child(if signed {
                let m = manage.clone();
                div()
                    .flex()
                    .gap(px(6.))
                    .child(Button::new("agent-manage", "Manage plan").small().with_icon("external-link").when(free, |b| b.primary()).on_click(move |_, _, _| folio_control::account::open_url(&m)))
                    .child(Button::new("agent-signout", "Sign out").small().ghost().with_icon("log-out").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("account.signOut", json!({}), cx))))
                    .into_any_element()
            } else {
                div().flex().gap(px(6.)).child(Button::new("agent-signin", "Sign in").small().primary().with_icon("log-in").on_click(|_, _, cx| Self::sign_in(cx))).into_any_element()
            })
            .into_any_element()
    }

    /// When the chosen provider (other than lsuite AI) can't run, or agents are off: why, and what to do.
    fn notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let kind = self.provider(cx);
        let (text, action) = if !s.settings.agent.permissions.enabled {
            ("Agents are turned off: the agent, MCP clients and folio-cli --agent are refused.".to_string(), None)
        } else if kind == ProviderKind::Lsuite {
            return None;
        } else {
            let st = self.status_of(kind).filter(|st| !st.ready)?;
            (st.message.clone(), st.action.clone())
        };
        let copy = action.as_ref().and_then(|a| a.command.clone());
        let link = action.as_ref().and_then(|a| a.url.clone());
        let label: SharedString = action.as_ref().map(|a| a.label.clone()).unwrap_or_default().into();
        Some(
            div()
                .flex_none()
                .m(px(12.))
                .mb_0()
                .p(px(10.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .border_1()
                .border_color(t.line_strong)
                .bg(t.bg_sunken.opacity(0.5))
                .text_size(px(sz::SM))
                .child(div().flex().gap(px(8.)).child(icon("info").mt(px(2.)).text_color(t.text_2)).child(div().flex_1().min_w_0().line_height(px(18.)).child(text)))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .when_some(link, |d, url| d.child(Button::new("notice-link", label.clone()).small().with_icon("external-link").on_click(move |_, _, _| folio_control::account::open_url(&url))))
                        .when_some(copy, |d, command| {
                            d.child(Button::new("notice-copy", label.clone()).small().with_icon("copy").on_click(move |_, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(command.clone()));
                                cx.store().update(cx, |s, cx| s.flash("Copied. Paste it in a terminal.", cx));
                            }))
                        })
                        .child(
                            Button::new("notice-check", if self.checking { "Checking…" } else { "Check again" })
                                .small()
                                .ghost()
                                .with_icon("refresh-cw")
                                .disabled(self.checking)
                                .on_click(cx.listener(|this, _, _, cx| this.refresh_statuses(cx))),
                        )
                        .child(Button::new("notice-settings", "Settings › Agent").small().ghost().on_click(|_, _, cx| Self::open_agent_settings(cx))),
                )
                .into_any_element(),
        )
    }

    fn empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .p(px(14.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().text_size(px(sz::MD)).font_weight(FontWeight::SEMIBOLD).child("Ask, and it's done in the file"))
                    .child(div().text_size(px(sz::SM)).line_height(px(18.)).text_color(t.text_2).child(
                        "The agent writes documents, fills sheets with formulas and builds decks with the same commands as the window, the CLI and MCP.",
                    ))
                    .child(div().text_size(px(sz::SM)).line_height(px(18.)).text_color(t.text_2).child(
                        "Every command shows here as a card, every change is one more step in the undo history, and a whole run can be reverted.",
                    )),
            )
            .child(
                div().flex().flex_col().gap(px(6.)).child(caps("Try", cx)).children(EXAMPLES.iter().enumerate().map(|(i, ex)| {
                    let text = ex.to_string();
                    div()
                        .id(("example", i))
                        .px(px(10.))
                        .py(px(7.))
                        .border_1()
                        .border_color(t.line)
                        .text_size(px(sz::SM))
                        .cursor_pointer()
                        .hover(|s| s.bg(t.hover).border_color(t.line_strong))
                        .child(ex.to_string())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let text = text.clone();
                            this.composer.update(cx, |i, cx| i.set_text(text, cx));
                            crate::ui::input::focus(&this.composer, window, cx);
                            cx.notify();
                        }))
                })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(caps("Permissions", cx))
                    .child(div().text_size(px(sz::SM)).line_height(px(18.)).text_color(t.text_2).child(
                        "Editing the open file is always allowed and always undoable. Files, settings, plugins and app control are each a switch, the same for this agent and MCP clients.",
                    ))
                    .child(div().flex().child(Button::new("perm-open", "Settings › Agent").small().with_icon("shield-check").on_click(|_, _, cx| Self::open_agent_settings(cx)))),
            )
    }

    fn running_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = cx.theme().clone();
        let a = self.snap.running.and_then(|id| self.snap.run(id))?;
        let secs = a.seconds() as u64;
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(sz::SM))
                .text_color(t.text_2)
                .child(spinner())
                .child(div().flex_1().min_w_0().truncate().child(a.activity.clone().unwrap_or_else(|| "Working…".into())))
                .child(div().font_family(MONO).text_size(px(sz::XS)).child(format!("{}:{:02}", secs / 60, secs % 60)))
                .into_any_element(),
        )
    }

    fn conversation_view(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.snap.entries.is_empty() && self.snap.running.is_none() {
            return div().id("agent-scroll").size_full().overflow_y_scroll().track_scroll(&self.scroll).child(self.empty_state(cx)).into_any_element();
        }
        let items: Vec<AnyElement> = self
            .snap
            .entries
            .iter()
            .enumerate()
            .map(|(i, item)| match item {
                Entry::User { text, source, .. } => self.user_bubble(i, text, *source, cx),
                Entry::Assistant { text, .. } => div().text_size(px(sz::BASE)).line_height(px(19.)).child(crate::ui::markdown::render(text, cx)).into_any_element(),
                Entry::Command { record, result, .. } => self.command_card(record, result.as_ref(), cx),
                Entry::Outcome { run, state, error, changes, seconds, tokens } => self.outcome_row(i, *run, *state, error.as_deref(), *changes, *seconds, *tokens, cx),
            })
            .collect();
        let running = self.running_row(cx);
        div()
            .id("agent-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(div().flex().flex_col().gap(px(8.)).p(px(12.)).children(items).children(running))
            .into_any_element()
    }

    /// A request; one sent from the CLI or an MCP client says so.
    fn user_bubble(&self, i: usize, text: &str, source: Source, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        div()
            .id(("user", i))
            .flex()
            .justify_end()
            .items_start()
            .gap(px(6.))
            .when(source != Source::Window, |d| d.child(div().mt(px(9.)).child(source_badge(source.as_str(), cx))))
            .child(
                div()
                    .max_w(gpui::relative(0.88))
                    .px(px(12.))
                    .py(px(8.))
                    .bg(t.accent)
                    .text_color(t.text_on_accent)
                    .text_size(px(sz::BASE))
                    .line_height(px(19.))
                    .child(text.to_string()),
            )
            .into_any_element()
    }

    /// One command: what ran, with what, whether it worked, who ran it and when. Click for the
    /// parameters and the answer.
    fn command_card(&self, record: &CommandRecord, result: Option<&Value>, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let seq = record.seq;
        let open = self.expanded.contains(&seq);
        let summary = params_summary(&record.params);
        let quiet = !record.mutates && record.ok;
        div()
            .id(("command", seq))
            .flex()
            .flex_col()
            .gap(px(3.))
            .px(px(10.))
            .py(px(7.))
            .bg(t.bg_sunken.opacity(if quiet { 0.3 } else { 0.55 }))
            .border_1()
            .border_color(if record.ok { t.line } else { t.danger })
            .cursor_pointer()
            .hover(|s| s.border_color(t.line_strong))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.expanded.remove(&seq) {
                    this.expanded.insert(seq);
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .child(icon(if record.ok { "check" } else { "circle-alert" }).size(px(12.)).text_color(if record.ok { t.text_2 } else { t.danger }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(MONO)
                            .text_size(px(sz::SM))
                            .font_weight(if quiet { FontWeight::NORMAL } else { FontWeight::SEMIBOLD })
                            .text_color(if quiet { t.text_2 } else { t.text })
                            .child(record.command.clone()),
                    )
                    .child(source_badge(record.source.as_str(), cx))
                    .child(div().flex_none().font_family(MONO).text_size(px(10.)).text_color(t.text_2).child(record.at.with_timezone(&chrono::Local).format("%H:%M:%S").to_string()))
                    .child(icon(if open { "chevron-up" } else { "chevron-down" }).size(px(12.)).text_color(t.text_2)),
            )
            .when(!summary.is_empty() && !open, |d| d.child(div().pl(px(19.)).truncate().font_family(MONO).text_size(px(10.5)).text_color(t.text_2).child(summary)))
            .when_some(record.error.clone(), |d, e| d.child(div().pl(px(19.)).text_size(px(sz::XS)).text_color(t.danger).child(e)))
            .when(open, |d| {
                let params = serde_json::to_string_pretty(&record.params).unwrap_or_default();
                let answer = result.or(record.result.as_ref()).map(|r| {
                    let s = serde_json::to_string_pretty(r).unwrap_or_default();
                    if s.chars().count() > RESULT_PREVIEW { format!("{}…", s.chars().take(RESULT_PREVIEW).collect::<String>()) } else { s }
                });
                d.child(code_block("Parameters", params, cx)).when_some(answer, |d, a| d.child(code_block("Answer", a, cx)))
            })
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn outcome_row(&self, i: usize, run: u64, state: RunState, error: Option<&str>, changes: usize, seconds: f64, tokens: u64, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let info: Option<&RunInfo> = self.snap.run(run);
        let can_revert = info.is_some_and(RunInfo::can_revert);
        let reverted = info.is_some_and(|r| r.reverted);
        let mut facts = vec![match changes {
            0 => "no changes".to_string(),
            1 => "1 change".to_string(),
            n => format!("{n} changes"),
        }];
        facts.push(format!("{:.0} s", seconds.max(1.0)));
        if tokens > 0 {
            facts.push(format!("{} tokens", short_count(tokens)));
        }
        let (ic, color, title): (&'static str, Hsla, SharedString) = match state {
            RunState::Done | RunState::Running => ("circle-check", t.text_2, "Done".into()),
            RunState::Error => ("circle-alert", t.danger, "Stopped on an error".into()),
            RunState::Cancelled => ("circle-stop", t.text_2, if changes > 0 { "Stopped; finished edits stay".into() } else { "Stopped".into() }),
        };
        let manage = error.and_then(folio_agent::lsuite::manage_url);
        let sign_in = error.is_some_and(|e| e == folio_agent::lsuite::SIGN_IN || e.starts_with("Sign in to lsuite AI"));
        div()
            .id(("outcome", i))
            .flex()
            .flex_col()
            .gap(px(6.))
            .when(state == RunState::Error, |d| d.p(px(10.)).border_1().border_color(t.danger))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(sz::SM))
                    .child(icon(ic).text_color(color))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().flex_1().min_w_0().truncate().text_color(t.text_2).child(facts.join(" · ")))
                    .when(can_revert, |d| {
                        d.child(
                            Button::new(("revert-run", i), "Revert this run")
                                .small()
                                .ghost()
                                .with_icon("rotate-ccw")
                                .tooltip("Put the file back as it was before this run (redo brings it back)")
                                .on_click(cx.listener(move |this, _, _, cx| this.revert_run(run, cx))),
                        )
                    })
                    .when(reverted, |d| d.child(div().flex_none().text_size(px(sz::XS)).text_color(t.text_2).child("Reverted"))),
            )
            .when_some(error.map(str::to_string), |d, m| d.child(div().text_size(px(sz::SM)).text_color(t.text).child(m)))
            .when(manage.is_some() || sign_in, |d| {
                d.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .when_some(manage, |d, url| d.child(Button::new(("outcome-manage", i), "Manage plan").small().primary().with_icon("external-link").on_click(move |_, _, _| folio_control::account::open_url(&url))))
                        .when(sign_in, |d| d.child(Button::new(("outcome-signin", i), "Sign in").small().primary().with_icon("log-in").on_click(|_, _, cx| Self::sign_in(cx)))),
                )
            })
            .into_any_element()
    }

    /// The panel's runs with "Revert this run", sessions from a terminal, and the shared undo
    /// history with who made each step.
    fn changes_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let history = self.history.clone().unwrap_or_default();
        let agent_steps = history.undo.iter().filter(|s| s.source != "window").count();
        let runs: Vec<RunInfo> = self.snap.runs.iter().rev().cloned().collect();
        let mut rows: Vec<AnyElement> = vec![];
        let mut mine = 0usize;
        let flush = |mine: &mut usize, rows: &mut Vec<AnyElement>| {
            if *mine > 0 {
                rows.push(
                    div()
                        .px(px(8.))
                        .py(px(4.))
                        .text_size(px(sz::XS))
                        .text_color(t.text_2)
                        .child(if *mine == 1 { "1 edit of yours".to_string() } else { format!("{} edits of yours", *mine) })
                        .into_any_element(),
                );
                *mine = 0;
            }
        };
        for (i, step) in history.undo.iter().enumerate().take(200) {
            if step.source == "window" {
                mine += 1;
                continue;
            }
            flush(&mut mine, &mut rows);
            rows.push(
                div()
                    .id(("step", i))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(5.))
                    .when(i == 0, |d| d.bg(t.hover))
                    .child(icon("history").size(px(12.)).text_color(t.text_2))
                    .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(sz::SM)).child(step.label.clone()))
                    .child(source_badge(&step.source, cx))
                    .into_any_element(),
            );
        }
        flush(&mut mine, &mut rows);

        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .p(px(12.))
            .child(div().text_size(px(sz::SM)).text_color(t.text_2).line_height(px(18.)).child(
                "Agents, MCP clients and the CLI edit through the same commands as you, into one undo history. Revert a whole run, or undo step by step.",
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(caps("Runs from this panel", cx))
                    .when(runs.is_empty(), |d| d.child(div().text_size(px(sz::SM)).text_color(t.text_2).child("None yet.")))
                    .children(runs.into_iter().map(|r| {
                        let i = r.id as usize;
                        let changes = match r.changes {
                            0 => "no changes".to_string(),
                            1 => "1 change".to_string(),
                            n => format!("{n} changes"),
                        };
                        div()
                            .id(("run", i))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .p(px(8.))
                            .bg(t.bg_sunken.opacity(0.5))
                            .border_1()
                            .border_color(t.line)
                            .child(logo(r.provider.id(), px(16.)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(div().truncate().text_size(px(sz::SM)).font_weight(FontWeight::MEDIUM).child(r.prompt.clone()))
                                    .child(div().truncate().text_size(px(sz::XS)).text_color(t.text_2).child(format!(
                                        "{} · {} · {}",
                                        r.started_at.with_timezone(&chrono::Local).format("%H:%M"),
                                        r.provider.label(),
                                        if r.finished() { changes } else { "running…".into() }
                                    ))),
                            )
                            .when(r.reverted, |d| d.child(div().text_size(px(sz::XS)).text_color(t.text_2).child("Reverted")))
                            .when(r.can_revert(), |d| {
                                let id = r.id;
                                d.child(Button::new(("changes-revert", i), "Revert this run").small().with_icon("rotate-ccw").on_click(cx.listener(move |this, _, _, cx| this.revert_run(id, cx))))
                            })
                    })),
            )
            .children(self.outside_sessions(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(caps(format!("Undo history · {agent_steps} by agents"), cx))
                            .child(crate::ui::group(
                                [
                                    Button::icon("changes-undo", "undo-2", "Undo")
                                        .small()
                                        .flush()
                                        .disabled(!history.can_undo)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let me = cx.entity().downgrade();
                                            this.store.update(cx, |s, cx| s.run_then("history.undo", json!({}), cx, move |_, _, cx| {
                                                me.update(cx, |p, cx| p.refresh_history(cx)).ok();
                                            }));
                                        }))
                                        .into_any_element(),
                                    Button::icon("changes-redo", "redo-2", "Redo")
                                        .small()
                                        .flush()
                                        .disabled(!history.can_redo)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let me = cx.entity().downgrade();
                                            this.store.update(cx, |s, cx| s.run_then("history.redo", json!({}), cx, move |_, _, cx| {
                                                me.update(cx, |p, cx| p.refresh_history(cx)).ok();
                                            }));
                                        }))
                                        .into_any_element(),
                                ],
                                cx,
                            )),
                    )
                    .when(self.history.is_none(), |d| d.child(div().text_size(px(sz::SM)).text_color(t.text_2).child("Open a file to see its history.")))
                    .when(self.history.is_some() && agent_steps == 0, |d| d.child(div().text_size(px(sz::SM)).text_color(t.text_2).child("No step by an agent, MCP or the CLI in this file's history.")))
                    .when(agent_steps > 0, |d| d.child(div().flex().flex_col().gap(px(1.)).children(rows)))
                    .when(!history.redo.is_empty(), |d| {
                        d.child(div().text_size(px(sz::XS)).text_color(t.text_2).child(format!("{} step{} can be redone.", history.redo.len(), if history.redo.len() == 1 { "" } else { "s" })))
                    }),
            )
            .into_any_element()
    }

    /// Agents working from a terminal (Claude Code, Codex, any MCP client): their commands carry the
    /// checkpoint the bridge took before their first change, so each session reverts in one click.
    fn outside_sessions(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = cx.theme().clone();
        let store = self.store.read(cx);
        // The built-in agent's own commands (Claude Code and Codex reach folio over MCP too)
        // belong to its runs, above.
        let ours: HashSet<u64> = self.snap.entries.iter().filter_map(|e| match e {
            Entry::Command { record, run: Some(_), .. } => Some(record.seq),
            _ => None,
        }).collect();
        let mut sessions: Vec<(u64, Source, usize, chrono::DateTime<chrono::Utc>)> = vec![];
        for r in store.commands.iter().filter(|r| r.mutates && r.ok && r.source != Source::Agent && !ours.contains(&r.seq)) {
            let Some(cp) = r.checkpoint else { continue };
            match sessions.iter_mut().find(|s| s.0 == cp) {
                Some(s) => s.2 += 1,
                None => sessions.push((cp, r.source, 1, r.at)),
            }
        }
        if sessions.is_empty() {
            return None;
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(caps("From a terminal", cx))
                .children(sessions.into_iter().rev().map(|(cp, source, n, at)| {
                    let reverted = self.reverted_sessions.contains(&cp);
                    div()
                        .id(("outside-session", cp as usize))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .p(px(8.))
                        .bg(t.bg_sunken.opacity(0.5))
                        .border_1()
                        .border_color(t.line)
                        .child(source_badge(source.as_str(), cx))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(sz::XS)).text_color(t.text_2).child(format!(
                            "since {} · {n} change{}",
                            at.with_timezone(&chrono::Local).format("%H:%M"),
                            if n == 1 { "" } else { "s" }
                        )))
                        .when(reverted, |d| d.child(div().text_size(px(sz::XS)).text_color(t.text_2).child("Reverted")))
                        .when(!reverted, |d| {
                            d.child(Button::new(("outside-revert", cp as usize), "Revert this session").small().with_icon("rotate-ccw").on_click(cx.listener(move |this, _, _, cx| this.revert_session(cp, cx))))
                        })
                }))
                .into_any_element(),
        )
    }

    /// The composer, in viewfinder brackets: what the agent will be told about the window, the
    /// request, and Send (Steer while a run goes on) with Stop.
    fn composer_view(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let focused = self.composer.read(cx).is_focused(window);
        let running = self.snap.running.is_some();
        let empty = self.composer.read(cx).text().trim().is_empty();
        let glance = folio_agent::glance(&self.store.read(cx).session);
        let detail: SharedString = format!("The agent is told what you see as you send:\n{}", glance.lines.iter().skip(1).cloned().collect::<Vec<_>>().join("\n")).into();
        div().flex_none().p(px(12.)).border_t_1().border_color(t.line).child(
            div()
                .relative()
                .p(px(6.))
                .bg(t.bg_sunken.opacity(if t.is_dark() { 0.55 } else { 0.7 }))
                .child(crate::ui::grain::brackets(10., 0., if focused { t.text } else { t.line_strong }))
                .child(
                    div()
                        .id("agent-glance")
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .px(px(6.))
                        .pt(px(2.))
                        .font_family(MONO)
                        .text_size(px(10.5))
                        .text_color(t.text_2)
                        .tooltip(move |_, cx| crate::ui::tooltip(detail.clone(), cx))
                        .child(icon("scan-eye").size(px(12.)))
                        .child(div().flex_1().min_w_0().truncate().child(glance.short.to_uppercase())),
                )
                .child(div().max_h(px(200.)).child(self.composer.clone()))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(4.))
                        .pb(px(2.))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(sz::XS)).text_color(t.text_2).child(if running { "Send to steer the run." } else { "Enter to send · Shift-Enter for a new line" }))
                        .when(running, |d| d.child(Button::icon("agent-stop", "square", "Stop").small().on_click(cx.listener(|this, _, _, cx| this.stop(cx)))))
                        .child(Button::new("agent-send", if running { "Steer" } else { "Send" }).small().primary().with_icon("arrow-up").disabled(empty).on_click(cx.listener(|this, _, _, cx| this.send(cx)))),
                ),
        )
    }
}

impl Render for AgentPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let header = self.header(cx).into_any_element();
        if self.host(cx).is_none() {
            return div().size_full().flex().flex_col().glass(t.glass1).border_0().border_l_1().border_color(t.line).child(header).child(
                div().p(px(14.)).text_size(px(sz::SM)).text_color(t.text_2).child("The built-in agent isn't running in this window. folio-mcp and folio-cli still work."),
            );
        }
        let lsuite = (self.provider(cx) == ProviderKind::Lsuite).then(|| self.lsuite_strip(cx));
        let notice = self.notice(cx);
        let agent_steps = self.history.as_ref().map(|h| h.undo.iter().filter(|s| s.source != "window").count()).unwrap_or(0);
        let this = cx.entity().downgrade();
        let tabs = segmented(
            "agent-tab",
            vec![(Tab::Conversation, "Conversation".into()), (Tab::Changes, if agent_steps > 0 { format!("Changes · {agent_steps}").into() } else { "Changes".into() })],
            self.tab,
            move |tab, _, cx| {
                let tab = *tab;
                this.update(cx, |p, cx| {
                    p.tab = tab;
                    if tab == Tab::Changes {
                        p.refresh_history(cx);
                    }
                    cx.notify();
                })
                .ok();
            },
            cx,
        );
        let body = match self.tab {
            Tab::Conversation => self.conversation_view(cx),
            Tab::Changes => div().id("agent-changes").size_full().overflow_y_scroll().track_scroll(&self.changes_scroll).child(self.changes_tab(cx)).into_any_element(),
        };
        let composer = (self.tab == Tab::Conversation).then(|| self.composer_view(window, cx).into_any_element());
        div()
            .size_full()
            .flex()
            .flex_col()
            .glass(t.glass1)
            .border_0()
            .border_l_1()
            .border_color(t.line)
            .text_size(px(sz::BASE))
            .child(header)
            .children(lsuite)
            .children(notice)
            .when_some(self.snap.storage_error.clone(), |d, e| d.child(div().flex_none().px(px(12.)).pt(px(8.)).text_size(px(sz::XS)).text_color(t.text_2).child(e)))
            .child(div().flex_none().px(px(12.)).pt(px(10.)).child(tabs))
            .child(div().flex_1().min_h_0().child(body))
            .children(composer)
    }
}

/// A turning loader (still when the system asks for less motion).
fn spinner() -> AnyElement {
    let i = icon("loader-circle").size(px(13.));
    if crate::theme::os_reduces_motion() {
        return i.into_any_element();
    }
    i.with_animation("agent-spin", gpui::Animation::new(Duration::from_millis(900)).repeat(), |svg, delta| svg.with_transformation(gpui::Transformation::rotate(gpui::percentage(delta))))
        .into_any_element()
}

/// A small caps badge naming who ran a command: AGENT, MCP, CLI or YOU. Agents and MCP are
/// inverted (ink on paper), the others outlined.
fn source_badge(source: &str, cx: &App) -> Div {
    let t = cx.theme();
    let ai = matches!(source, "agent" | "mcp");
    let label = if source == "window" { "you" } else { source };
    div()
        .flex_none()
        .px(px(5.))
        .py(px(1.))
        .border_1()
        .border_color(if ai { t.accent } else { t.line_strong })
        .when(ai, |d| d.bg(t.accent).text_color(t.text_on_accent))
        .when(!ai, |d| d.text_color(t.text_2))
        .font_family(MONO)
        .text_size(px(9.5))
        .child(label.to_uppercase())
}

/// `key="value", n=3, ids=[2]`: a one-line view of a command's parameters.
fn params_summary(params: &Value) -> String {
    let Some(o) = params.as_object() else { return String::new() };
    let mut out = vec![];
    for (k, v) in o {
        let shown = match v {
            Value::Null => continue,
            Value::String(s) => {
                let short: String = s.chars().take(28).collect();
                if short.len() < s.len() { format!("\"{short}…\"") } else { format!("\"{s}\"") }
            }
            Value::Number(n) => {
                let f = n.as_f64().unwrap_or(0.0);
                if f.fract() == 0.0 { format!("{f:.0}") } else { format!("{f:.2}").trim_end_matches('0').trim_end_matches('.').to_string() }
            }
            Value::Bool(b) => b.to_string(),
            Value::Array(a) => format!("[{}]", a.len()),
            Value::Object(m) => format!("{{{}}}", m.len()),
        };
        out.push(format!("{k}={shown}"));
    }
    out.join(", ")
}

fn code_block(title: &str, body: String, cx: &App) -> AnyElement {
    let t = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap(px(3.))
        .mt(px(4.))
        .child(caps(title.to_string(), cx))
        .child(
            div()
                .p(px(8.))
                .bg(t.bg_sunken.opacity(0.8))
                .border_1()
                .border_color(t.line)
                .font_family(MONO)
                .text_size(px(10.5))
                .line_height(px(15.))
                .text_color(t.text)
                .children(body.lines().map(|l| div().child(if l.is_empty() { " ".to_string() } else { l.to_string() }))),
        )
        .into_any_element()
}

/// `1.2k`, `830`.
fn short_count(n: u64) -> String {
    if n >= 1000 { format!("{:.1}k", n as f64 / 1000.0) } else { n.to_string() }
}

/// How much an entry holds, to notice the last one growing (streamed text).
fn entry_len(e: &Entry) -> usize {
    match e {
        Entry::Assistant { text, .. } => text.len(),
        Entry::Command { result, .. } => usize::from(result.is_some()),
        _ => 0,
    }
}
