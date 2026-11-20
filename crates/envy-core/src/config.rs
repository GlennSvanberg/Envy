//! `%APPDATA%\envy\config.toml` (and the XDG equivalent off Windows).
//!
//! The file stores folders, display-name overrides, and settings. It has no
//! field for environment values, and [`save`] only serializes this struct.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeSetting {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub theme: ThemeSetting,
    /// When true, a copied value is wiped from the clipboard after 30 seconds
    /// if it is still there. When false, the clipboard is left alone.
    #[serde(default = "default_clear_clipboard")]
    pub clear_clipboard: bool,
    /// Chord such as `ctrl+alt+e`. Parsed by [`crate::hotkey::Hotkey`].
    #[serde(default = "default_hotkey")]
    pub global_hotkey: String,
}

fn default_clear_clipboard() -> bool {
    true
}

fn default_hotkey() -> String {
    "ctrl+alt+e".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeSetting::System,
            clear_clipboard: true,
            global_hotkey: default_hotkey(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub folders: Vec<PathBuf>,
    #[serde(default)]
    pub settings: Settings,
    /// Deeplink repo name → project root. Used when two folders share a name.
    #[serde(default)]
    pub projects: BTreeMap<String, PathBuf>,
    /// Project root → display name, when the git remote name is not what you want.
    #[serde(default)]
    pub project_names: BTreeMap<String, String>,
}

impl Config {
    pub fn load() -> Self {
        Self::load_from(&config_path())
    }

    pub fn load_from(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(&config_path())
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        fs::write(path, text)
    }

    pub fn add_folder(&mut self, folder: PathBuf) {
        if !self.folders.iter().any(|existing| existing == &folder) {
            self.folders.push(folder);
        }
    }

    pub fn remove_folder(&mut self, folder: &Path) {
        self.folders.retain(|existing| existing != folder);
    }
}

pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| {
            std::env::var("USERPROFILE")
                .map(|profile| format!("{profile}\\AppData\\Roaming"))
                .unwrap_or_else(|_| ".".into())
        });
        return PathBuf::from(base).join("envy");
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return PathBuf::from(xdg).join("envy");
            }
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".config").join("envy")
    }
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_has_no_value_field() {
        let mut config = Config::default();
        config.folders.push(PathBuf::from("/work/app"));
        config.settings.theme = ThemeSetting::Dark;
        config.settings.clear_clipboard = false;
        config
            .projects
            .insert("billing".into(), PathBuf::from("/work/billing"));
        config
            .project_names
            .insert("/work/billing".into(), "Billing API".into());
        let dir = std::env::temp_dir().join(format!("envy-config-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        config.save_to(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("clear_clipboard = false"));
        assert!(text.contains("theme = \"dark\""));
        assert!(!text.contains("value"));
        assert!(!text.contains("SECRET"));
        let loaded = Config::load_from(&path);
        assert_eq!(loaded, config);
        let _ = fs::remove_dir_all(&dir);
    }
}
