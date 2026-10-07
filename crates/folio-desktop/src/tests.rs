//! The real window in GPUI's headless test platform: a session with a file, the workspace as
//! the root view, simulated typing and keys. Commands run on Tokio as in the app.

use std::sync::Arc;
use std::time::{Duration, Instant};

use folio_control::{Session, SessionOptions, Source};
use gpui::{TestAppContext, VisualTestContext};
use serde_json::{Value, json};

use crate::app::Workspace;
use crate::store::StoreExt;

struct Fixture {
    rt: tokio::runtime::Runtime,
    _dir: tempfile::TempDir,
    session: Arc<Session>,
}

impl Fixture {
    fn call(&self, name: &str, params: Value) -> Value {
        self.rt.block_on(folio_control::call(&self.session, Source::Cli, name, params)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }
}

const PATIENCE: Duration = Duration::from_secs(20);

#[cfg(target_os = "macos")]
const M: &str = "cmd";
#[cfg(not(target_os = "macos"))]
const M: &str = "ctrl";

fn settle(cx: &mut VisualTestContext, done: impl Fn(&crate::store::Store) -> bool) {
    let start = Instant::now();
    while !cx.update(|_, cx| done(cx.store().read(cx))) && start.elapsed() < PATIENCE {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn setup<'a>(cx: &'a mut TestAppContext, kind: &str) -> (Fixture, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let session = {
        let _g = rt.enter();
        Session::new(SessionOptions { data_dir: Some(dir.path().join("data")), config_dir: Some(dir.path().join("config")), secrets: None, headless: false }).unwrap()
    };
    let f = Fixture { rt, _dir: dir, session };
    f.call("file.new", json!({ "kind": kind }));
    let (handle, session) = (f.rt.handle().clone(), f.session.clone());
    cx.update(|cx| {
        gpui_tokio::init_from_handle(cx, handle);
        crate::app::init(session, cx);
    });
    let (_view, vcx) = cx.add_window_view(Workspace::new);
    vcx.run_until_parked();
    settle(vcx, |s| s.doc.is_some());
    vcx.run_until_parked();
    (f, vcx)
}

#[gpui::test]
fn typing_in_a_document_edits_through_the_registry(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, "doc");
    cx.simulate_input("Hello world");
    cx.run_until_parked();
    let text = f.call("page.text", json!({}));
    assert_eq!(text["text"], "Hello world");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("Second");
    cx.run_until_parked();
    let d = f.call("doc.read", json!({}));
    assert_eq!(d["blocks"][1]["text"], "Second");
    // Typing is one undo step per burst, and undo goes through the same history.
    cx.simulate_keystrokes(&format!("{M}-z"));
    cx.run_until_parked();
    let text = f.call("page.text", json!({}));
    assert!(text["text"].as_str().unwrap().len() < "Hello world\nSecond".len());
    let steps = f.call("history.list", json!({}));
    assert_eq!(steps["redo"][0]["source"], "window");
}

#[gpui::test]
fn bold_applies_to_the_selection(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, "doc");
    cx.simulate_input("make me bold");
    cx.simulate_keystrokes(&format!("{M}-a"));
    cx.simulate_keystrokes(&format!("{M}-b"));
    cx.run_until_parked();
    let d = f.call("doc.read", json!({}));
    assert_eq!(d["blocks"][0]["runs"][0]["bold"], true, "{d}");
}

#[gpui::test]
fn typing_in_a_sheet_commits_cells(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, "sheet");
    cx.simulate_input("2");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("3");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("=A1*A2");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let r = f.call("sheet.read", json!({}));
    assert_eq!(r["rows"][2][0], json!(6.0), "{r}");
    // The active cell moved down after each one.
    let ui = f.call("ui.state", json!({}));
    assert_eq!(ui["cell"], "A4");
}

#[gpui::test]
fn decks_add_slides_and_shapes(cx: &mut TestAppContext) {
    let (f, cx) = setup(cx, "deck");
    cx.simulate_keystrokes(&format!("{M}-shift-m"));
    cx.run_until_parked();
    let d = f.call("deck.read", json!({}));
    assert_eq!(d["slides"].as_array().unwrap().len(), 2);
}
