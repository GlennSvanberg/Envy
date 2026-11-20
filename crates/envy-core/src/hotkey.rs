//! Parse a user-written hotkey such as `ctrl+alt+e`.
//!
//! The string is stored in config. Turning it into an OS registration is the
//! app's job; this module only checks that the chord is well formed.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub mods: Mods,
    /// Lowercase key name: `e`, `f1`, `space`.
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeyError {
    pub message: String,
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HotkeyError {}

impl Hotkey {
    pub fn parse(input: &str) -> Result<Self, HotkeyError> {
        let parts: Vec<&str> = input
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();
        if parts.is_empty() {
            return Err(HotkeyError {
                message: "hotkey is empty".into(),
            });
        }
        let mut mods = Mods {
            ctrl: false,
            alt: false,
            shift: false,
            super_key: false,
        };
        let key = parts[parts.len() - 1].to_ascii_lowercase();
        if !is_key(&key) {
            return Err(HotkeyError {
                message: format!("'{key}' is not a key"),
            });
        }
        for part in &parts[..parts.len() - 1] {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" => mods.ctrl = true,
                "alt" | "opt" | "option" => mods.alt = true,
                "shift" => mods.shift = true,
                "super" | "win" | "meta" => mods.super_key = true,
                other => {
                    return Err(HotkeyError {
                        message: format!("'{other}' is not a modifier"),
                    });
                }
            }
        }
        if !(mods.ctrl || mods.alt || mods.shift || mods.super_key) {
            return Err(HotkeyError {
                message: "hotkey needs a modifier so it does not steal typing".into(),
            });
        }
        Ok(Self { mods, key })
    }

    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.mods.ctrl {
            parts.push("ctrl");
        }
        if self.mods.alt {
            parts.push("alt");
        }
        if self.mods.shift {
            parts.push("shift");
        }
        if self.mods.super_key {
            parts.push("super");
        }
        parts.push(self.key.as_str());
        parts.join("+")
    }
}

fn is_key(key: &str) -> bool {
    if key.len() == 1 {
        let c = key.chars().next().unwrap();
        return c.is_ascii_alphanumeric();
    }
    matches!(
        key,
        "space"
            | "enter"
            | "tab"
            | "escape"
            | "esc"
            | "backspace"
            | "delete"
            | "up"
            | "down"
            | "left"
            | "right"
            | "f1"
            | "f2"
            | "f3"
            | "f4"
            | "f5"
            | "f6"
            | "f7"
            | "f8"
            | "f9"
            | "f10"
            | "f11"
            | "f12"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_chord() {
        let hotkey = Hotkey::parse("ctrl+alt+e").unwrap();
        assert!(hotkey.mods.ctrl && hotkey.mods.alt);
        assert_eq!(hotkey.key, "e");
        assert_eq!(hotkey.display(), "ctrl+alt+e");
    }

    #[test]
    fn rejects_unmodified_letter() {
        assert!(Hotkey::parse("e").is_err());
    }
}
