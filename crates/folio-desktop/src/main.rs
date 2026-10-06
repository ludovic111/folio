//! folio: the desktop window (GPUI) around the document model.
//!
//! The window is one client of the command registry in `folio-control`, like the built-in agent,
//! `folio-cli` and `folio-mcp`; it starts the session, the loopback bridge those tools use, and
//! writes the lsuite discovery file.

// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod assets;
mod paint;
mod store;
mod theme;
mod ui;
mod views;

#[cfg(test)]
mod tests;

use gpui::{App, AppContext as _, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions, point, px, size};
use folio_control::{Session, SessionOptions};

fn main() {
    init_logging();
    // Background work (saving, the bridge, the agent) runs on Tokio; GPUI drives the window.
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(3).enable_all().thread_name("folio-worker").build().expect("tokio runtime");
    let session = {
        let _guard = runtime.enter();
        Session::new(SessionOptions { secrets: Some(folio_control::secrets::default_store()), headless: false, ..Default::default() }).unwrap_or_else(|e| fatal(&format!("folio couldn't create its folders: {e}")))
    };

    // The bridge for folio-cli and folio-mcp, and the lsuite discovery entry.
    let bridge = runtime.block_on(folio_control::bridge::Server::start(session.clone()));
    let running = match &bridge {
        Ok(server) => Some(folio_control::discovery::Running { pid: std::process::id(), control_file: Some(server.path().to_path_buf()), port: Some(server.port()), since: chrono::Utc::now() }),
        Err(e) => {
            tracing::warn!("the control bridge couldn't start: {e}");
            None
        }
    };
    if let Err(e) = folio_control::discovery::write(&folio_control::discovery::entry(&session.data_dir, running)) {
        tracing::warn!("couldn't write ~/.lsuite/apps/folio.json: {e}");
    }

    // Logging out or shutting down sends SIGTERM: save, say so in the discovery file, stop.
    #[cfg(unix)]
    {
        let s = session.clone();
        runtime.spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};
            let (Ok(mut term), Ok(mut hup), Ok(mut int)) = (signal(SignalKind::terminate()), signal(SignalKind::hangup()), signal(SignalKind::interrupt())) else { return };
            tokio::select! {
                _ = term.recv() => {}
                _ = hup.recv() => {}
                _ = int.recv() => {}
            }
            s.flush();
            let _ = folio_control::discovery::write(&folio_control::discovery::entry(&s.data_dir, None));
            std::process::exit(0);
        });
    }

    // A file given on the command line (or by the OS) opens at start.
    let open_at_start: Option<std::path::PathBuf> = std::env::args().skip(1).find(|a| !a.starts_with('-')).map(std::path::PathBuf::from);

    let handle = runtime.handle().clone();
    let bridge = parking_lot::Mutex::new(bridge.ok());
    gpui_platform::application().with_assets(assets::Assets).run(move |cx: &mut App| {
        gpui_tokio::init_from_handle(cx, handle);
        assets::load_fonts(cx);
        app::init(session.clone(), cx);
        open_main_window(cx);
        cx.activate(true);
        app::start(&session, open_at_start.clone(), cx);
        let s = session.clone();
        cx.on_app_quit(move |_| {
            s.flush();
            let _ = folio_control::discovery::write(&folio_control::discovery::entry(&s.data_dir, None));
            drop(bridge.lock().take());
            async {}
        })
        .detach();
    });
    drop(runtime);
}

/// Logs to stderr at the level `RUST_LOG` asks for (info by default).
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,naga=warn,wgpu=warn"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).try_init();
}

/// Before the window exists: log it, say it on stderr and stop.
fn fatal(message: &str) -> ! {
    tracing::error!("{message}");
    eprintln!("{message}");
    std::process::exit(1)
}

/// Smallest window that keeps every area usable.
pub const WINDOW_MIN_W: f32 = 900.;
pub const WINDOW_MIN_H: f32 = 600.;

pub fn open_main_window(cx: &mut App) {
    // `FOLIO_WINDOW_SIZE=2000x1250` opens the window at that size (screenshots, tests).
    let (w, h) = std::env::var("FOLIO_WINDOW_SIZE")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))))
        .unwrap_or_else(|| {
            let screen = cx.primary_display().map(|d| d.visible_bounds().size);
            let fit = |want: f32, room: Option<f32>, min: f32| room.map_or(want, |r| want.min(r * 0.92)).max(min);
            (fit(1440., screen.map(|s| f32::from(s.width)), WINDOW_MIN_W), fit(900., screen.map(|s| f32::from(s.height)), WINDOW_MIN_H))
        });
    let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
    let transparent = cx.global::<theme::Theme>().transparent;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions { title: Some("folio".into()), appears_transparent: true, traffic_light_position: Some(point(px(16.), px(17.))) }),
        focus: true,
        show: true,
        window_min_size: Some(size(px(WINDOW_MIN_W), px(WINDOW_MIN_H))),
        window_background: if transparent { WindowBackgroundAppearance::Blurred } else { WindowBackgroundAppearance::Opaque },
        app_id: Some("folio".into()),
        icon: image::load_from_memory(include_bytes!("../resources/folio.png")).ok().map(|i| std::sync::Arc::new(i.to_rgba8())),
        ..Default::default()
    };
    cx.open_window(options, |window, cx| cx.new(|cx| app::Workspace::new(window, cx))).expect("couldn't open the folio window");
}
