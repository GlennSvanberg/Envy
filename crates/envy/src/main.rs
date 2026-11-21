//! Envy — a keyboard-first editor for `.env` files.
//!
//! Closing the window quits. A second launch forwards its deeplink to the
//! process that is already running.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod hotkey_os;
mod instance;
mod protocol;
mod view;
mod watch;

use std::sync::Arc;
use std::time::Instant;

use gpui_kit::{App, AppContext};
use view::EnvyApp;

pub(crate) fn window_icon() -> Option<Arc<image::RgbaImage>> {
    let bytes = include_bytes!("../../../assets/icon/envy-icon-64.png");
    image::load_from_memory(bytes)
        .ok()
        .map(|image| Arc::new(image.into_rgba8()))
}

/// Process start, before any window work. Used to measure time-to-first-frame.
pub(crate) static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn main() {
    let _ = STARTED.set(Instant::now());
    let args: Vec<String> = std::env::args().skip(1).collect();
    let payload = args
        .iter()
        .find(|arg| arg.starts_with("envy:"))
        .cloned()
        .unwrap_or_else(|| "activate".into());
    // The second process connects and exits here, before GPUI starts, so it
    // never opens a window. The listener itself runs on its own thread.
    let server = instance::claim_or_forward(&payload);
    std::thread::Builder::new()
        .name("envy-protocol".into())
        .spawn(|| {
            if let Err(err) = protocol::register() {
                note(&format!("URL scheme was not registered: {err}"));
            }
        })
        .ok();

    let launch = args.into_iter().find(|arg| arg.starts_with("envy:"));
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            actions::bind(cx);
            let options = EnvyApp::window_options(cx);
            let launch = launch.clone();
            let server = server;
            // `server` is moved; the closure is FnOnce.
            let opened = gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| EnvyApp::new(server, launch, window, cx))
            });
            match opened {
                Ok((window, view)) => {
                    window
                        .update(cx, |_, window, cx| {
                            let view = view.clone();
                            window.on_window_should_close(cx, move |_, cx| {
                                view.update(cx, |this, cx| this.close_requested(cx))
                            });
                            window.set_window_title("Envy");
                        })
                        .ok();
                }
                Err(err) => note(&format!("Could not open a window: {err}")),
            }
        });
}

pub(crate) fn note(message: &str) {
    let path = envy_core::config_dir().join("startup.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = writeln!(file, "{message}");
    }
}
