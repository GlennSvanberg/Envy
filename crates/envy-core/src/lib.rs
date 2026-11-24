//! File-side core for Envy.
//!
//! This crate never opens a window. It parses `.env` files without dropping
//! bytes it did not change, diffs them, finds them under folders the user
//! added, and reads or writes config that contains paths and settings only.
//!
//! Secret values live in process memory while a file is open. They are not
//! written to config or logs. A save uses a short-lived sibling file only long
//! enough to rename it into place. Callers must not log [`Entry::value`].

pub mod cells;
mod chrome;
pub mod columns;
pub mod config;
pub mod deeplink;
pub mod diff;
pub mod discover;
pub mod document;
pub mod future;
pub mod hotkey;
pub mod interact;
mod labels;
pub mod names;
pub mod paste;
pub mod pathfmt;
pub mod scaffold;
pub mod search;
pub mod session;
pub mod suggest;
pub mod text_field;

pub use cells::{
    display_filename_origin, display_key_origin, display_value_origin, edit_filename_origin,
    edit_key_origin, edit_value_origin, TextOrigin, KEY_CELL_GAP_PX, KEY_CELL_PAD_RIGHT_PX,
    KEY_LINE_PX, KEY_RELATED_ICON_PX, KEY_RELATED_PAD_PX, KEY_ROW_H_PX, KEY_TEXT_PX,
    KEY_TOGGLE_W_PX, ROW_BORDER_PX, ROW_PAD_X_PX, TAB_DOT_PX, TAB_GAP_PX, TAB_H_PX, TAB_LINE_PX,
    TAB_PAD_X_PX, TAB_TEXT_PX, VALUE_PAD_RIGHT_PX,
};
pub use chrome::{
    centered_search_frame, fit_toolbar, key_search_frame, project_search_frame, search_field_inset,
    SearchFrame, ToolbarFit, KEY_SEARCH_MIN_PX, KEY_SEARCH_PREFERRED_PX, PROJECT_HEADER_H_PX,
    SEARCH_BAND_BORDER_PX, SEARCH_BAND_H_PX, SEARCH_FIELD_H_PX, SEARCH_LINE_PX, SEARCH_TEXT_PX,
    TAB_BAR_H_PX, TOOLBAR_H_PX, TOOLBAR_NOTICE_MIN_PX, TOOLBAR_NOTICE_SHRINK,
    TOOLBAR_SEARCH_SHRINK,
};
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
    enter_while_suggesting, is_cell_commit_key, route_cell_edit, route_value_edit, row_click,
    search_landing, search_mark, step_index, CellMotion, CellRoute, EditCell, EnterEffect,
    RowClick, RowTarget, ValueMotion, ValueRoute, TAB_MENU_LEN,
};
pub use labels::{labeled_name, relative_slash_path, tab_labels, unique_dir_suffixes, TabLabel};
pub use names::{
    check_env_filename, env_name_taken, next_env_filename, normalize_env_filename, rename_env_file,
    EnvFileNameError,
};
pub use paste::{apply_paste, plan_paste, PasteChange, PastePlan};
pub use pathfmt::{
    abbreviate_home, host_path_style, native_project_path, sidebar_path_chars, truncate_path_head,
    PathStyle,
};
pub use scaffold::{copied_document, copy_source_order, copy_values_for, from_example, CopyValues};
pub use search::{search, SearchHit, SearchKind};
pub use session::{inspect_disk, save_text, DiskNotice, OpenFile, SAVE_PAUSE};
pub use suggest::{common_env_keys, match_span, suggest_keys, KeySuggestion, SuggestionSource};
pub use text_field::{first_line, TextField};
