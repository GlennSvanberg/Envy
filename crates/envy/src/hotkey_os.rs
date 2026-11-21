//! OS-level global hotkey. The chord itself is parsed and validated in
//! `envy-core`; this module only registers it.
//!
//! Registration runs on a dedicated thread. On Linux the `global-hotkey`
//! crate blocks the caller until X11 answers, which would freeze the window.
//! On Windows `RegisterHotKey` delivers `WM_HOTKEY` to the thread that created
//! the hidden window, so that thread also pumps messages.
//!
//! Linux support in the `global-hotkey` crate is X11. Wayland sessions will
//! fail registration and Envy keeps running without the shortcut.

#[cfg(windows)]
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use std::thread;
#[cfg(windows)]
use std::time::Duration;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

struct Registration {
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
}

enum Command {
    Replace(String, Sender<Result<(), String>>),
}

static SERVICE: OnceLock<Sender<Command>> = OnceLock::new();

/// Ask the hotkey thread to register `spec`. The returned receiver is filled
/// when registration finishes. Waiting on it belongs on a background task.
pub fn request(spec: String) -> Receiver<Result<(), String>> {
    let (reply_tx, reply_rx) = mpsc::channel();
    match SERVICE.get_or_init(start) {
        tx => {
            if tx.send(Command::Replace(spec, reply_tx)).is_err() {
                let (fallback_tx, fallback_rx) = mpsc::channel();
                let _ = fallback_tx.send(Err("Hotkey thread stopped.".into()));
                return fallback_rx;
            }
        }
    }
    reply_rx
}

pub fn pressed() -> bool {
    let mut hit = false;
    while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
        if event.state == HotKeyState::Pressed {
            hit = true;
        }
    }
    hit
}

fn start() -> Sender<Command> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("envy-hotkey".into())
        .spawn(move || hotkey_thread(rx))
        .ok();
    tx
}

fn hotkey_thread(rx: Receiver<Command>) {
    let mut slot = None;
    #[cfg(windows)]
    {
        loop {
            pump_messages();
            match rx.try_recv() {
                Ok(Command::Replace(spec, reply)) => {
                    let _ = reply.send(replace(&mut slot, &spec));
                }
                Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(15)),
                Err(TryRecvError::Disconnected) => break,
            }
        }
    }
    #[cfg(not(windows))]
    {
        while let Ok(Command::Replace(spec, reply)) = rx.recv() {
            let _ = reply.send(replace(&mut slot, &spec));
        }
    }
}

fn replace(slot: &mut Option<Registration>, spec: &str) -> Result<(), String> {
    if let Some(current) = slot.take() {
        let _ = current.manager.unregister(current.hotkey);
    }
    *slot = Some(install(spec)?);
    Ok(())
}

fn install(spec: &str) -> Result<Registration, String> {
    let parsed = envy_core::Hotkey::parse(spec).map_err(|err| err.to_string())?;
    let hotkey: HotKey = parsed
        .display()
        .parse()
        .map_err(|err: global_hotkey::hotkey::HotKeyParseError| err.to_string())?;
    let manager = GlobalHotKeyManager::new().map_err(|err| err.to_string())?;
    manager.register(hotkey).map_err(|err| err.to_string())?;
    Ok(Registration { manager, hotkey })
}

/// `WM_HOTKEY` is posted to this thread. Drain the queue so the callback runs.
#[cfg(windows)]
fn pump_messages() {
    #[repr(C)]
    struct Msg {
        hwnd: *mut core::ffi::c_void,
        message: u32,
        _pad: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt_x: i32,
        pt_y: i32,
    }
    extern "system" {
        fn PeekMessageW(
            msg: *mut Msg,
            hwnd: *mut core::ffi::c_void,
            min: u32,
            max: u32,
            remove: u32,
        ) -> i32;
        fn TranslateMessage(msg: *const Msg) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> isize;
    }
    const PM_REMOVE: u32 = 0x0001;
    unsafe {
        loop {
            let mut msg = std::mem::zeroed();
            if PeekMessageW(&mut msg, core::ptr::null_mut(), 0, 0, PM_REMOVE) == 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
