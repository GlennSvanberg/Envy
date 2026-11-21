//! File-side core for Envy.
//!
//! This crate never opens a window. It parses `.env` files without dropping
//! bytes it did not change, diffs them, finds them under folders the user
//! added, and reads or writes config that contains paths and settings only.
//!
//! Secret values live in process memory while a file is open. They are not
//! written to config or logs. A save uses a short-lived sibling file only long
//! enough to rename it into place. Callers must not log [`Entry::value`].

pub mod columns;
pub mod config;
pub mod deeplink;
pub mod diff;
pub mod discover;
pub mod document;
pub mod future;
pub mod hotkey;
pub mod interact;
pub mod names;
pub mod paste;
pub mod pathfmt;
pub mod scaffold;
pub mod search;
pub mod session;
pub mod suggest;
pub mod text_field;

pub use columns::{
    clamp_key_column_px, clamp_sidebar_px, fitted_key_column_px, KEY_COLUMN_MAX_PX,
    KEY_COLUMN_MIN_PX, SIDEBAR_DEFAULT_PX, SIDEBAR_MAX_PX, SIDEBAR_MIN_PX,
};
pub use config::{
    config_dir, config_path, sidebar_rank, Config, EditorView, Settings, ThemeSetting,
};
pub use deeplink::Deeplink;
pub use diff::{DiffKind, DiffReport, DiffRow};
pub use discover::{
    attach_git_identity, cache_from_projects, discover, discover_files, fill_key_index,
    is_env_filename, is_skipped_dir, load_index_cache, projects_from_cache, save_index_cache,
    scan_paths, upsert_file, EnvFile, FoundEnv, IndexCache, Project, ScanError, MAX_SCAN_DEPTH,
};
pub use document::{
    classify_verbatim, is_env_key, DocumentLayout, Entry, EnvDocument, KeySnapshot, LayoutItem,
    Node, QuoteStyle, UnreadableLine, VerbatimKind,
};
pub use hotkey::{Hotkey, HotkeyError};
pub use interact::{
    route_cell_edit, route_value_edit, row_click, CellMotion, CellRoute, EditCell, RowClick,
    RowTarget, ValueMotion, ValueRoute,
};
pub use names::{
    check_env_filename, env_name_taken, next_env_filename, normalize_env_filename, rename_env_file,
    EnvFileNameError,
};
pub use paste::{apply_paste, plan_paste, PasteChange, PastePlan};
pub use pathfmt::{
    abbreviate_home, host_path_style, native_project_path, sidebar_path_chars, truncate_path_head,
    PathStyle,
};
pub use scaffold::from_example;
pub use search::{search, SearchHit, SearchKind};
pub use session::{inspect_disk, save_text, DiskNotice, OpenFile};
pub use suggest::{common_env_keys, match_span, suggest_keys, KeySuggestion, SuggestionSource};
pub use text_field::{first_line, TextField};
