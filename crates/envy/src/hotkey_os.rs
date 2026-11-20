//! OS-level global hotkey. The chord itself is parsed and validated in
//! `envy-core`; this module only registers it.
//!
//! Linux support in the `global-hotkey` crate is X11. Wayland sessions will
//! fail registration and Envy keeps running without the shortcut.

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

pub struct Registration {
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
}

pub fn install(spec: &str) -> Result<Registration, String> {
    let parsed = envy_core::Hotkey::parse(spec).map_err(|err| err.to_string())?;
    // `ctrl+alt+e` is a form global-hotkey accepts (`E` == `KeyE`).
    let hotkey: HotKey = parsed
        .display()
        .parse()
        .map_err(|err: global_hotkey::hotkey::HotKeyParseError| err.to_string())?;
    let manager = GlobalHotKeyManager::new().map_err(|err| err.to_string())?;
    manager.register(hotkey).map_err(|err| err.to_string())?;
    Ok(Registration { manager, hotkey })
}

pub fn replace(slot: &mut Option<Registration>, spec: &str) -> Result<(), String> {
    if let Some(current) = slot.take() {
        let _ = current.manager.unregister(current.hotkey);
    }
    *slot = Some(install(spec)?);
    Ok(())
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
