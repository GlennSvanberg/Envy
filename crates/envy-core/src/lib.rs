//! File-side core for Envy.
//!
//! This crate never opens a window. It parses `.env` files without dropping
//! bytes it did not change, diffs them, finds them under folders the user
//! added, and reads or writes config that contains paths and settings only.
//!
//! Secret values live in process memory while a file is open. They are not
//! written to config or logs. A save uses a short-lived sibling file only long
//! enough to rename it into place. Callers must not log [`Entry::value`].
//!
//! Later, a CLI or an encrypted-share tool can depend on this crate directly.
//! Those features are named in [`future`] and are not implemented.

pub mod config;
pub mod deeplink;
pub mod diff;
pub mod discover;
pub mod document;
pub mod future;
pub mod hotkey;
pub mod paste;
pub mod scaffold;
pub mod search;
pub mod session;

pub use config::{config_dir, config_path, Config, Settings, ThemeSetting};
pub use deeplink::Deeplink;
pub use diff::{DiffKind, DiffReport, DiffRow};
pub use discover::{discover, EnvFile, Project, ScanError};
pub use document::{is_env_key, Entry, EnvDocument, Node, QuoteStyle};
pub use hotkey::{Hotkey, HotkeyError};
pub use paste::{apply_paste, plan_paste, PasteChange, PastePlan};
pub use scaffold::from_example;
pub use search::{search, SearchHit, SearchKind};
pub use session::OpenFile;
