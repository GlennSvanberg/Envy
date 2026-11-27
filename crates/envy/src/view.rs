//! The Envy window.
//!
//! Projects on the left, the open file on the right. Values stay in memory
//! and are not written to the status line.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use envy_core::discover::{
    default_file_index, is_example_filename, is_prod_filename, resolve_file, resolve_project,
};
use envy_core::document::{LayoutItem, Node, UnreadableLine};
use envy_core::paste::{PasteChange, PastePlan};
use envy_core::{
    abbreviate_home, attach_git_identity, cache_from_projects, check_env_filename,
    clamp_key_column_px, clamp_sidebar_px, copied_document, copy_source_order, copy_values_for,
    diff, enter_while_suggesting, fill_key_index, fitted_key_column_px, host_path_style,
    is_cell_commit_key, is_env_key, labeled_name, load_index_cache, match_span,
    native_project_path, next_env_filename, plan_paste, projects_from_cache, relative_slash_path,
    rename_env_file, route_cell_edit, row_click, save_index_cache, save_text, scan_paths,
    search_field_inset, search_landing, search_mark, sidebar_path_chars, sidebar_rank, step_index,
    suggest_keys, tab_labels, truncate_path_head, upsert_file, CellMotion, Config, CopyValues,
    Deeplink, EditCell, EditorView, EnterEffect, EnvDocument, EnvFile, EnvFileNameError, FoundEnv,
    KeySuggestion, OpenFile, PathStyle, Project, RowClick, RowTarget, ScanError, ThemeSetting,
    KEY_CELL_GAP_PX, KEY_CELL_PAD_RIGHT_PX, KEY_LINE_PX, KEY_RELATED_ICON_PX, KEY_RELATED_PAD_PX,
    KEY_SEARCH_MIN_PX, KEY_SEARCH_PREFERRED_PX, KEY_TEXT_PX, KEY_TOGGLE_W_PX, SAVE_PAUSE,
    SEARCH_BAND_BORDER_PX, SEARCH_BAND_H_PX, SEARCH_FIELD_H_PX, SEARCH_LINE_PX, SEARCH_TEXT_PX,
    TAB_DOT_PX, TAB_GAP_PX, TAB_H_PX, TAB_LINE_PX, TAB_MENU_LEN, TAB_PAD_X_PX, TAB_TEXT_PX,
    TOOLBAR_NOTICE_MIN_PX, TOOLBAR_NOTICE_SHRINK, TOOLBAR_SEARCH_SHRINK, VALUE_PAD_RIGHT_PX,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Editor as CodeEditor, EditorState, InputEvent};
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Disableable, Icon, Selectable, Sizable, Theme, ThemeMode, TitleBar,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, size, AnyElement, AnyView, AnyWindowHandle, App, Bounds, ClipboardItem, Context,
    Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent,
    PathPromptOptions, ScrollHandle, SharedString, Subscription, Task, Window, WindowBounds,
    WindowOptions,
};

use crate::actions::*;
use crate::copy_feedback::{
    copy_icon_button, row_copy_id, CopyAck, CopyAckBook, CopyButtonKind, TOOLBAR_COPY_ID,
};
use crate::dotenv_highlight::DotenvHighlighter;
use crate::hotkey_os;
use crate::instance::Instance;
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{
    self, Palette, COL_HEADER_H, DIFF_GUTTER_W, DIFF_HEADER_H, DIFF_KEY_COL_W, DIFF_ROW_H,
    FOOTER_H, KEY_ACTIONS_W, KEY_ROW_H, MASK_MAX_DOTS, SECTION_ROW_H, SIDEBAR_HEADER_H,
    SIDEBAR_ROW_BRANCH_H, SIDEBAR_W, TAB_BAR_H, TITLE_BAR_H, TOOLBAR_H,
};

const CLEAR_AFTER: Duration = Duration::from_secs(30);
const SAVED_FLASH_AFTER: Duration = Duration::from_millis(1200);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone {
    Projects,
    Tabs,
    Keys,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffSide {
    Left,
    Right,
}

enum EditKind {
    Value,
    Rename,
    New,
}

struct Editor {
    key: Option<Entity<TextInput>>,
    value: Entity<TextInput>,
    node: Option<usize>,
    kind: EditKind,
    suggestions: Vec<KeySuggestion>,
    suggestions_open: bool,
    /// Document from the moment the cell opened. Live edits undo back to this.
    baseline: EnvDocument,
    /// [`OpenFile::arm_undo`] has pushed `baseline`.
    undo_armed: bool,
    /// Row inserted for a new key, hidden while that editor is still open.
    preview_node: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SavePhase {
    Idle,
    Saving,
    Saved,
    Failed,
}

#[derive(Clone)]
struct FailedSave {
    path: PathBuf,
    text: String,
}

enum Overlay {
    None,
    Settings,
    AddFolder,
    Paste(PastePlan),
    Shortcuts,
    Unreadable,
    DeleteFile { file_ix: usize, name: String },
}

struct TabMenu {
    file_ix: usize,
    cursor: usize,
}

enum FilePrompt {
    None,
    Create {
        input: Entity<TextInput>,
        error: Option<String>,
        /// `0` is an empty file. `1..` indexes [`copy_source_order`].
        source: usize,
        /// Used for a non-example source. Example files always keep their values.
        keys_only: bool,
        source_focus: bool,
    },
    Rename {
        file_ix: usize,
        input: Entity<TextInput>,
        error: Option<String>,
    },
}

enum NewFileBody {
    Empty,
    Copy { source: PathBuf, values: CopyValues },
}

#[derive(Clone, Copy)]
enum ResizeKind {
    Sidebar,
    KeyColumn,
}

struct ResizeDrag {
    kind: ResizeKind,
    origin_x: f32,
    origin_w: f32,
}

struct VisibleLayout {
    items: Vec<LayoutItem>,
    entry_positions: Vec<usize>,
    entry_nodes: Vec<usize>,
}

pub struct EnvyApp {
    focus: FocusHandle,
    window: AnyWindowHandle,
    instance: Option<Instance>,
    config: Config,
    projects: Vec<Project>,
    scan_errors: Vec<ScanError>,
    project_ix: Option<usize>,
    file_ix: Option<usize>,
    tab_cursor: usize,
    /// Recent order captured at launch. Navigation writes config for the next
    /// launch and does not reorder this list. Pinning does.
    sort_recent: Vec<PathBuf>,
    open: Option<OpenFile>,
    zone: Zone,
    /// Stay in Find a project until the user clicks or types, including
    /// while a restored file is still opening.
    focus_project_on_open: bool,
    cursor: usize,
    revealed: HashSet<String>,
    show_all: bool,
    editor: Option<Editor>,
    edit_subs: Vec<Subscription>,
    overlay: Overlay,
    folder_input: Entity<TextInput>,
    hotkey_input: Entity<TextInput>,
    input_subs: Vec<Subscription>,
    project_scroll: ScrollHandle,
    key_scroll: ScrollHandle,
    diff_scroll: ScrollHandle,
    project_filter: Entity<TextInput>,
    filter_query: String,
    /// Sidebar row highlighted after Down, before Enter opens that project.
    project_mark: Option<usize>,
    key_filter: Entity<TextInput>,
    key_query: String,
    editor_view: EditorView,
    text_editor: Entity<EditorState>,
    text_editor_syncing: bool,
    text_editor_needs_sync: bool,
    text_session_before: Option<EnvDocument>,
    /// Bumped whenever a plain-text edit should wait again before writing.
    text_save_gen: u64,
    /// Bumped whenever a cell edit should wait again before writing.
    cell_save_gen: u64,
    /// Bumped when the cell editor is replaced, so a late blur cannot commit the next one.
    edit_gen: u64,
    save_phase: SavePhase,
    save_failed: Option<FailedSave>,
    /// A save job is on the background executor right now.
    save_active: bool,
    /// Bumped so a stale "Saved" timer cannot clear a newer one.
    saved_flash: u64,
    reload_keep_cursor: bool,
    diff_open: bool,
    diff_cursor: usize,
    diff_left: Option<usize>,
    diff_right: Option<usize>,
    diff_left_doc: Option<EnvDocument>,
    diff_right_doc: Option<EnvDocument>,
    last_dark: Option<bool>,
    status: String,
    /// Copy buttons that are still showing the check. Keyed by button id.
    copy_acks: CopyAckBook,
    clear_token: u64,
    clear_task: Option<Task<()>>,
    poll_task: Option<Task<()>>,
    /// Config has not been read yet. The first frame paints before that.
    booting: bool,
    scanning: bool,
    scan_found: usize,
    scan_again: bool,
    watch: Option<crate::watch::FolderWatch>,
    scan_epoch: u64,
    io_epoch: u64,
    save_seq: u64,
    io_queue: VecDeque<IoJob>,
    io_running: bool,
    diff_epoch: u64,
    diff_report: Option<diff::DiffReport>,
    diff_stale: bool,
    disk_check: bool,
    last_disk: Instant,
    config_dirty: bool,
    config_saving: bool,
    pending_launch: Option<String>,
    pending_file: Option<PathBuf>,
    pending_key: Option<String>,
    file_prompt: FilePrompt,
    file_prompt_sub: Option<Subscription>,
    tab_menu: Option<TabMenu>,
    sidebar_width: f32,
    /// `None` until the divider is dragged. The column then fits the longest key.
    key_column_width: Option<f32>,
    resize_drag: Option<ResizeDrag>,
    startup_logged: bool,
    ui_ready: Duration,
    config_load: Duration,
}

enum ScanMsg {
    Found(FoundEnv),
    Done(Vec<ScanError>),
}

enum IoJob {
    Save {
        seq: u64,
        path: PathBuf,
        text: String,
    },
    Load {
        epoch: u64,
        path: PathBuf,
    },
    Reload {
        epoch: u64,
        path: PathBuf,
    },
    Delete {
        path: PathBuf,
        name: String,
    },
    Quit,
}

impl EnvyApp {
    pub fn new(
        instance: Option<Instance>,
        launch: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let folder_input = cx.new(|cx| TextInput::new("Folder path", cx));
        let hotkey_input = cx.new(|cx| TextInput::new("ctrl+alt+e", cx));
        let project_filter = cx.new(|cx| TextInput::new("Find a project", cx).exit_down());
        let key_filter = cx.new(|cx| TextInput::new("Find a key", cx).exit_down());
        let text_editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .line_number(true)
                .folding(false)
                .auto_close(false)
                .smart_indent(false)
                .language("dotenv");
            state
                .set_highlighter_factory(Rc::new(|_| Some(Box::new(DotenvHighlighter::new()))), cx);
            state
        });
        let focus = cx.focus_handle();
        let now = Instant::now();
        let mut app = Self {
            focus: focus.clone(),
            window: window.window_handle(),
            instance,
            config: Config::default(),
            projects: Vec::new(),
            scan_errors: Vec::new(),
            project_ix: None,
            file_ix: None,
            tab_cursor: 0,
            sort_recent: Vec::new(),
            open: None,
            zone: Zone::Projects,
            focus_project_on_open: true,
            cursor: 0,
            revealed: HashSet::new(),
            show_all: false,
            editor: None,
            edit_subs: Vec::new(),
            overlay: Overlay::None,
            folder_input,
            hotkey_input,
            input_subs: Vec::new(),
            project_scroll: ScrollHandle::new(),
            key_scroll: ScrollHandle::new(),
            diff_scroll: ScrollHandle::new(),
            project_filter,
            filter_query: String::new(),
            project_mark: None,
            key_filter,
            key_query: String::new(),
            editor_view: EditorView::Table,
            text_editor,
            text_editor_syncing: false,
            text_editor_needs_sync: false,
            text_session_before: None,
            text_save_gen: 0,
            cell_save_gen: 0,
            edit_gen: 0,
            save_phase: SavePhase::Idle,
            save_failed: None,
            save_active: false,
            saved_flash: 0,
            reload_keep_cursor: false,
            diff_open: false,
            diff_cursor: 0,
            diff_left: None,
            diff_right: None,
            diff_left_doc: None,
            diff_right_doc: None,
            last_dark: None,
            status: "Starting".into(),
            copy_acks: CopyAckBook::default(),
            clear_token: 0,
            clear_task: None,
            poll_task: None,
            booting: true,
            scanning: false,
            scan_found: 0,
            scan_again: false,
            watch: None,
            scan_epoch: 0,
            io_epoch: 0,
            save_seq: 0,
            io_queue: VecDeque::new(),
            io_running: false,
            diff_epoch: 0,
            diff_report: None,
            diff_stale: false,
            disk_check: false,
            last_disk: now,
            config_dirty: false,
            config_saving: false,
            pending_launch: launch,
            pending_file: None,
            pending_key: None,
            file_prompt: FilePrompt::None,
            file_prompt_sub: None,
            tab_menu: None,
            sidebar_width: SIDEBAR_W.as_f32(),
            key_column_width: None,
            resize_drag: None,
            startup_logged: false,
            ui_ready: crate::STARTED
                .get()
                .map(|started| started.elapsed())
                .unwrap_or_default(),
            config_load: Duration::ZERO,
        };
        app.wire_inputs(window, cx);
        app.poll_task = Some(app.spawn_poll(cx));
        app.spawn_boot(cx);
        let focus = app.focus.clone();
        cx.defer_in(window, move |this, window, cx| {
            let handle = this.project_filter.focus_handle(cx);
            window.focus(&handle, cx);
            let _ = focus;
        });
        app
    }

    /// Read config after the first frame, then scan. Neither blocks painting.
    fn spawn_boot(&mut self, cx: &mut Context<Self>) {
        let ui_ready = crate::STARTED
            .get()
            .map(|started| started.elapsed())
            .unwrap_or_default();
        cx.spawn(async move |this, async_cx| {
            let loaded = async_cx
                .background_executor()
                .spawn(async move {
                    let began = Instant::now();
                    let config = Config::load();
                    let cached = projects_from_cache(&config.folders, &load_index_cache());
                    (config, cached, began.elapsed())
                })
                .await;
            let _ = this.update(async_cx, |view, cx| {
                view.finish_boot(loaded.0, loaded.1, ui_ready, loaded.2, cx);
            });
        })
        .detach();
    }

    fn finish_boot(
        &mut self,
        config: Config,
        cached: Vec<Project>,
        ui_ready: Duration,
        config_load: Duration,
        cx: &mut Context<Self>,
    ) {
        self.config = config;
        self.editor_view = self.config.settings.editor_view;
        if let Some(width) = self.config.settings.sidebar_width {
            self.sidebar_width = clamp_sidebar_px(width as f32);
        }
        if let Some(width) = self.config.settings.key_column_width {
            self.key_column_width = Some(clamp_key_column_px(width as f32));
        }
        self.sort_recent = self.config.recent_projects.clone();
        self.booting = false;
        let hotkey = self.config.settings.global_hotkey.clone();
        let window_handle = self.window;
        let hotkey_input = self.hotkey_input.clone();
        let theme_setting = self.config.settings.theme;
        window_handle
            .update(cx, |_, window, cx| {
                let hotkey = hotkey.clone();
                hotkey_input.update(cx, |input, cx| {
                    input.set_value(hotkey, window, cx);
                });
                apply_theme_setting(theme_setting, window, cx);
            })
            .ok();
        self.last_dark = Some(cx.theme().mode.is_dark());
        self.install_hotkey(hotkey, false, cx);
        self.config_load = config_load;
        let _ = ui_ready;
        if self.config.folders.is_empty() {
            self.log_startup(None, None);
            self.status = "Add a folder to start. Ctrl+O".into();
            self.hold_open_focus(cx);
            cx.notify();
            return;
        }
        if !cached.is_empty() {
            self.projects = cached;
            self.project_ix = self.visible_project_indexes().first().copied();
            self.ensure_open(cx);
        }
        self.arm_watch();
        self.begin_scan(false, cx);
        self.hold_open_focus(cx);
        cx.notify();
    }

    pub fn window_options(bounds_cx: &mut App) -> WindowOptions {
        let bounds = Bounds::centered(None, size(px(1120.), px(720.)), bounds_cx);
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitleBar::title_bar_options()),
            app_owns_titlebar_drag: true,
            app_id: Some("envy".into()),
            window_min_size: Some(size(px(780.), px(500.))),
            focus: true,
            show: true,
            icon: crate::window_icon(),
            ..Default::default()
        }
    }

    fn spawn_poll(&self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, async_cx| loop {
            async_cx
                .background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let _ = async_cx.update(|cx| {
                let _ = this.update(cx, |view, cx| view.poll(cx));
            });
        })
    }

    fn wire_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input_subs.push(cx.subscribe_in(
            &self.folder_input,
            window,
            |this, state, event, _window, cx| {
                if let TextInputEvent::Submit = event {
                    let path = state.read(cx).value().to_string();
                    this.commit_folder(path, cx);
                }
            },
        ));
        self.input_subs.push(cx.subscribe_in(
            &self.project_filter,
            window,
            |this, state, event, window, cx| match event {
                TextInputEvent::Change => {
                    this.focus_project_on_open = false;
                    this.project_mark = None;
                    this.filter_query = state.read(cx).value().to_string();
                    if let Some(index) = this.marked_project(true) {
                        this.scroll_project_index(index);
                    }
                    cx.notify();
                }
                TextInputEvent::Submit => {
                    this.focus_project_on_open = false;
                    this.open_filtered_project(window, cx);
                }
                TextInputEvent::ExitDown => {
                    this.focus_project_on_open = false;
                    this.leave_search(Zone::Projects, window, cx);
                }
                TextInputEvent::Focus | TextInputEvent::Blur => {}
            },
        ));
        self.input_subs.push(cx.subscribe_in(
            &self.key_filter,
            window,
            |this, state, event, window, cx| match event {
                TextInputEvent::Change => {
                    this.focus_project_on_open = false;
                    this.key_query = state.read(cx).value().to_string();
                    let len = this.visible_entry_nodes().len();
                    if !this.key_query.trim().is_empty() {
                        this.cursor = 0;
                    } else if this.cursor >= len {
                        this.cursor = 0;
                    }
                    if let Some(&item_ix) = this.visible_layout().entry_positions.get(this.cursor) {
                        this.key_scroll.scroll_to_item(item_ix);
                    }
                    cx.notify();
                }
                TextInputEvent::Submit => {
                    this.focus_project_on_open = false;
                    let len = this.visible_entry_nodes().len();
                    if !this.key_query.trim().is_empty() && len > 0 {
                        this.cursor = 0;
                    }
                    this.zone = Zone::Keys;
                    this.begin_value_edit(window, cx);
                    if this.editor.is_none() {
                        this.focus_self(window, cx);
                    }
                    cx.notify();
                }
                TextInputEvent::ExitDown => this.leave_search(Zone::Keys, window, cx),
                TextInputEvent::Focus | TextInputEvent::Blur => {}
            },
        ));
        self.input_subs.push(cx.subscribe_in(
            &self.hotkey_input,
            window,
            |this, state, event, window, cx| {
                if let TextInputEvent::Submit = event {
                    let spec = state.read(cx).value().to_string();
                    this.commit_hotkey(spec, window, cx);
                }
            },
        ));
        self.input_subs.push(cx.subscribe_in(
            &self.text_editor,
            window,
            |this, state, event, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    if this.text_editor_syncing || this.editor_view != EditorView::Text {
                        return;
                    }
                    let text = state.read(cx).value().to_string();
                    if let Some(open) = this.open.as_mut() {
                        if this.text_session_before.is_none() {
                            this.text_session_before = Some(open.document.clone());
                        }
                        if open.set_text_live(&text) {
                            this.mark_edited(cx);
                            cx.notify();
                        }
                    }
                }
            },
        ));
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        if hotkey_os::pressed() {
            self.raise(cx);
        }
        if let Some(instance) = &self.instance {
            let messages = instance.poll();
            for message in messages {
                self.on_second_launch(message, cx);
            }
        }
        self.schedule_disk_check(cx);
        if self.watch.as_ref().is_some_and(|watch| watch.pinged()) {
            if self.scanning {
                self.scan_again = true;
            } else if !self.booting {
                self.begin_scan(true, cx);
            }
        }
        if self.diff_stale {
            self.diff_stale = false;
            self.recompute_diff(cx);
        }
        if self.config.settings.theme == ThemeSetting::System {
            Theme::sync_system_appearance(None, cx);
        }
        let dark = cx.theme().mode.is_dark();
        if self.last_dark != Some(dark) {
            self.last_dark = Some(dark);
            theme::apply_palette(cx);
        }
    }

    fn on_second_launch(&mut self, message: String, cx: &mut Context<Self>) {
        self.raise(cx);
        if message == "activate" {
            return;
        }
        match Deeplink::parse(&message) {
            Ok(link) => self.follow_link_cx(link, cx),
            Err(err) => self.status = err.to_string(),
        }
        cx.notify();
    }

    fn raise(&self, cx: &mut App) {
        let focus = self.focus.clone();
        self.window
            .update(cx, |_, window, cx| {
                window.activate_window();
                cx.activate(true);
                window.focus(&focus, cx);
            })
            .ok();
    }

    fn apply_theme(&mut self, window: &mut Window, cx: &mut App) {
        apply_theme_setting(self.config.settings.theme, window, cx);
        self.last_dark = Some(cx.theme().mode.is_dark());
    }

    fn begin_scan(&mut self, quiet: bool, cx: &mut Context<Self>) {
        if self.config.folders.is_empty() {
            self.scanning = false;
            self.scan_found = 0;
            self.projects.clear();
            self.scan_errors.clear();
            cx.notify();
            return;
        }
        self.scan_epoch = self.scan_epoch.wrapping_add(1);
        let epoch = self.scan_epoch;
        self.scanning = true;
        self.scan_found = 0;
        self.scan_again = false;
        if !quiet && self.status.starts_with("Add a folder") {
            self.status.clear();
        }
        let folders = self.config.folders.clone();
        let config = self.config.clone();
        cx.notify();
        cx.spawn(async move |this, async_cx| {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut seen = HashSet::new();
            let started = Instant::now();
            let folders_for_scan = folders.clone();
            async_cx
                .background_executor()
                .spawn(async move {
                    let errors = scan_paths(&folders_for_scan, |found| {
                        let _ = tx.send(ScanMsg::Found(found));
                    });
                    let _ = tx.send(ScanMsg::Done(errors));
                })
                .detach();
            loop {
                let alive = this
                    .update(async_cx, |view, _cx| view.scan_epoch == epoch)
                    .unwrap_or(false);
                if !alive {
                    break;
                }
                let mut found = Vec::new();
                let mut done = None;
                loop {
                    match rx.try_recv() {
                        Ok(ScanMsg::Found(file)) => found.push(file),
                        Ok(ScanMsg::Done(errors)) => {
                            done = Some(errors);
                            break;
                        }
                        Err(_) => break,
                    }
                }
                for file in &found {
                    seen.insert(file.path.clone());
                }
                if !found.is_empty() {
                    let _ = this.update(async_cx, |view, cx| {
                        if view.scan_epoch != epoch {
                            return;
                        }
                        view.ingest_found(found, cx);
                    });
                }
                if let Some(errors) = done {
                    let files_took = started.elapsed();
                    let mut projects = this
                        .update(async_cx, |view, _cx| view.projects.clone())
                        .unwrap_or_default();
                    for project in &mut projects {
                        project.files.retain(|file| seen.contains(&file.path));
                    }
                    projects.retain(|project| !project.files.is_empty());
                    let config_for_git = config.clone();
                    let git_started = Instant::now();
                    let named = async_cx
                        .background_executor()
                        .spawn(async move {
                            fill_key_index(&mut projects);
                            attach_git_identity(&mut projects, &config_for_git);
                            projects
                        })
                        .await;
                    let git_took = git_started.elapsed();
                    let folders_for_cache = folders.clone();
                    let cache = cache_from_projects(&folders_for_cache, &named);
                    async_cx
                        .background_executor()
                        .spawn(async move {
                            let _ = save_index_cache(&cache);
                        })
                        .detach();
                    let _ = this.update(async_cx, |view, cx| {
                        if view.scan_epoch != epoch {
                            return;
                        }
                        view.apply_projects(named, errors);
                        view.scanning = false;
                        view.arm_watch();
                        view.ensure_open(cx);
                        view.open_pending_file(cx);
                        view.follow_pending_launch(cx);
                        if !quiet {
                            view.log_startup(Some(files_took), Some(git_took));
                        }
                        cx.notify();
                        if view.scan_again {
                            view.begin_scan(true, cx);
                        }
                    });
                    break;
                }
                async_cx
                    .background_executor()
                    .timer(Duration::from_millis(32))
                    .await;
            }
        })
        .detach();
    }

    fn ingest_found(&mut self, found: Vec<FoundEnv>, cx: &mut Context<Self>) {
        for file in found {
            let prod = is_prod_filename(&file.name);
            upsert_file(
                &mut self.projects,
                file.root,
                EnvFile {
                    path: file.path,
                    name: file.name,
                    keys: Vec::new(),
                    active_keys: Vec::new(),
                    prod,
                    missing_vs_example: false,
                },
            );
            self.scan_found = self.scan_found.saturating_add(1);
        }
        if self.project_ix.is_none() && !self.projects.is_empty() {
            self.project_ix = self.visible_project_indexes().first().copied();
            self.file_ix = self
                .project_ix
                .and_then(|index| self.projects.get(index))
                .and_then(default_file_index);
        }
        self.ensure_open(cx);
        self.open_pending_file(cx);
        cx.notify();
    }

    fn arm_watch(&mut self) {
        let mut paths = self.config.folders.clone();
        for project in &self.projects {
            if !paths.iter().any(|path| path == &project.root) {
                paths.push(project.root.clone());
            }
        }
        self.watch = Some(crate::watch::spawn(paths));
    }

    fn apply_projects(&mut self, projects: Vec<Project>, errors: Vec<ScanError>) {
        let root = self
            .project_ix
            .and_then(|index| self.projects.get(index))
            .map(|project| project.root.clone());
        let file = self.current_path();
        self.projects = projects;
        self.scan_errors = errors;
        self.project_ix = root
            .as_ref()
            .and_then(|root| {
                self.projects
                    .iter()
                    .position(|project| &project.root == root)
            })
            .or_else(|| self.visible_project_indexes().first().copied());
        if let (Some(project_ix), Some(file)) = (self.project_ix, file) {
            self.file_ix = self.projects[project_ix]
                .files
                .iter()
                .position(|item| item.path == file);
        }
    }

    fn current_path(&self) -> Option<PathBuf> {
        let project = self.projects.get(self.project_ix?)?;
        Some(project.files.get(self.file_ix?)?.path.clone())
    }

    fn entry_nodes(&self) -> Vec<usize> {
        self.open
            .as_ref()
            .map(|open| open.document.entries().map(|(index, _)| index).collect())
            .unwrap_or_default()
    }

    fn visible_layout(&self) -> VisibleLayout {
        let Some(open) = &self.open else {
            return VisibleLayout {
                items: Vec::new(),
                entry_positions: Vec::new(),
                entry_nodes: Vec::new(),
            };
        };
        let layout = open.document.layout();
        let query = self.key_query.trim().to_ascii_lowercase();
        let mut items = Vec::with_capacity(layout.items.len());
        let mut entry_positions = Vec::new();
        let mut entry_nodes = Vec::new();

        if query.is_empty() {
            for item in layout.items {
                if let LayoutItem::Entry { node_index, .. } = &item {
                    entry_positions.push(items.len());
                    entry_nodes.push(*node_index);
                }
                items.push(item);
            }
        } else {
            let mut pending_section: Option<LayoutItem> = None;
            for item in layout.items {
                match item {
                    LayoutItem::Section { .. } => {
                        pending_section = Some(item);
                    }
                    LayoutItem::Entry {
                        node_index,
                        ref description,
                        ..
                    } => {
                        let matched = match open.document.nodes.get(node_index) {
                            Some(Node::Entry(entry)) => {
                                fuzzy_hit(&entry.key, &query)
                                    || description
                                        .as_deref()
                                        .is_some_and(|desc| fuzzy_hit(desc, &query))
                            }
                            _ => false,
                        };
                        if matched {
                            if let Some(section) = pending_section.take() {
                                items.push(section);
                            }
                            entry_positions.push(items.len());
                            entry_nodes.push(node_index);
                            items.push(item);
                        }
                    }
                }
            }
        }

        VisibleLayout {
            items,
            entry_positions,
            entry_nodes,
        }
    }

    fn visible_entry_nodes(&self) -> Vec<usize> {
        self.visible_layout().entry_nodes
    }

    fn selected_node(&self) -> Option<usize> {
        self.visible_entry_nodes().get(self.cursor).copied()
    }

    fn selected_key(&self) -> Option<String> {
        let node = self.selected_node()?;
        match self.open.as_ref()?.document.nodes.get(node)? {
            Node::Entry(entry) => Some(entry.key.clone()),
            Node::Verbatim(_) => None,
        }
    }

    fn select_project(
        &mut self,
        index: usize,
        focus_keys: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.projects.len() {
            return;
        }
        self.project_mark = None;
        let changed = self.project_ix != Some(index);
        self.project_ix = Some(index);
        self.file_ix = default_file_index(&self.projects[index]);
        if focus_keys {
            self.zone = Zone::Keys;
        }
        if changed {
            self.clear_compare();
        }
        self.load_selected_file(cx);
        if changed && self.diff_open {
            self.load_diff(cx);
        }
        self.scroll_project_into_view();
        self.focus_self(window, cx);
        cx.notify();
    }

    fn remember_project(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(root) = self.projects.get(index).map(|project| project.root.clone()) else {
            return;
        };
        let before = self.config.recent_projects.clone();
        self.config.note_recent(&root);
        if self.config.recent_projects != before {
            self.persist_config(cx);
        }
    }

    fn visible_project_indexes(&self) -> Vec<usize> {
        let query = self.filter_query.trim().to_ascii_lowercase();
        let mut indexes: Vec<usize> = self
            .projects
            .iter()
            .enumerate()
            .filter(|(_, project)| {
                if query.is_empty() {
                    return true;
                }
                let raw = project.root.display().to_string();
                let native = native_project_path(&raw, host_path_style());
                let pretty = displayed_project_path(&native);
                fuzzy_hit(&project.name, &query)
                    || fuzzy_hit(&raw, &query)
                    || fuzzy_hit(&native, &query)
                    || fuzzy_hit(&pretty, &query)
            })
            .map(|(index, _)| index)
            .collect();
        let favorites = self.config.favorites.clone();
        let recent = self.sort_recent.clone();
        indexes.sort_by(|&left, &right| {
            let a = &self.projects[left];
            let b = &self.projects[right];
            sidebar_rank(&a.root, &a.name, &favorites, &recent)
                .cmp(&sidebar_rank(&b.root, &b.name, &favorites, &recent))
                .then(left.cmp(&right))
        });
        indexes
    }

    fn scroll_project_into_view(&self) {
        let Some(index) = self.project_ix else {
            return;
        };
        self.scroll_project_index(index);
    }

    fn scroll_project_index(&self, index: usize) {
        let Some(position) = self.project_visual_index(index) else {
            return;
        };
        self.project_scroll.scroll_to_item(position);
    }

    /// Row the sidebar paints as selected.
    ///
    /// Typing marks the first hit and leaves the open project alone. Down
    /// keeps that hit highlighted until Enter opens it.
    fn marked_project(&self, filter_focused: bool) -> Option<usize> {
        let visible = self.visible_project_indexes();
        let query_active = !self.filter_query.trim().is_empty();
        if filter_focused {
            return search_mark(query_active, self.project_ix, &visible);
        }
        if self.zone == Zone::Projects {
            if let Some(mark) = self.project_mark {
                if visible.contains(&mark) {
                    return Some(mark);
                }
            }
        }
        self.project_ix.filter(|index| visible.contains(index))
    }

    /// Child index in the project list, counting the section headers.
    fn project_visual_index(&self, target: usize) -> Option<usize> {
        let visible = self.visible_project_indexes();
        let mut visual = 0usize;
        let mut saw_favorite = false;
        let mut saw_other = false;
        for index in visible {
            let favorite = self
                .projects
                .get(index)
                .is_some_and(|project| self.config.is_favorite(&project.root));
            if favorite && !saw_favorite {
                visual += 1;
                saw_favorite = true;
            }
            if !favorite && !saw_other {
                visual += 1;
                saw_other = true;
            }
            if index == target {
                return Some(visual);
            }
            visual += 1;
        }
        None
    }

    fn focus_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.zone = Zone::Projects;
        self.scroll_project_into_view();
        self.focus_self(window, cx);
        cx.notify();
    }

    fn focus_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_count() == 0 {
            self.focus_keys_zone(window, cx);
            return;
        }
        self.tab_cursor = self.file_ix.unwrap_or(0).min(self.file_count() - 1);
        self.zone = Zone::Tabs;
        self.focus_self(window, cx);
        cx.notify();
    }

    fn file_count(&self) -> usize {
        self.project_ix
            .and_then(|ix| self.projects.get(ix))
            .map(|project| project.files.len())
            .unwrap_or(0)
    }

    fn on_left(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.nudge_copy_mode(true, cx) {
            return;
        }
        if self.tab_menu.is_some() || !matches!(self.overlay, Overlay::None) {
            return;
        }
        match self.zone {
            Zone::Tabs => self.move_tab(-1, cx),
            Zone::Keys | Zone::Projects => self.focus_sidebar(window, cx),
        }
    }

    fn on_right(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.nudge_copy_mode(false, cx) {
            return;
        }
        if self.tab_menu.is_some() || !matches!(self.overlay, Overlay::None) {
            return;
        }
        match self.zone {
            Zone::Tabs => self.move_tab(1, cx),
            Zone::Projects => self.focus_tabs(window, cx),
            Zone::Keys => self.focus_keys_zone(window, cx),
        }
    }

    fn switch_zone(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.overlay, Overlay::None) {
            if forward {
                window.focus_next(cx);
            } else {
                window.focus_prev(cx);
            }
            return;
        }
        if self.editor.is_some() {
            self.apply_cell_motion(
                if forward {
                    CellMotion::NextCell
                } else {
                    CellMotion::PrevCell
                },
                window,
                cx,
            );
            return;
        }
        let has_tabs = self.file_count() > 0;
        self.zone = match (self.zone, forward, has_tabs) {
            (Zone::Projects, true, true) => Zone::Tabs,
            (Zone::Projects, true, false) => Zone::Keys,
            (Zone::Tabs, true, _) => Zone::Keys,
            (Zone::Keys, true, _) => Zone::Projects,
            (Zone::Projects, false, _) => Zone::Keys,
            (Zone::Keys, false, true) => Zone::Tabs,
            (Zone::Keys, false, false) => Zone::Projects,
            (Zone::Tabs, false, _) => Zone::Projects,
        };
        if self.zone == Zone::Projects {
            self.scroll_project_into_view();
        }
        if self.zone == Zone::Tabs {
            let count = self.file_count();
            if count > 0 {
                self.tab_cursor = self.file_ix.unwrap_or(0).min(count - 1);
            }
        }
        self.focus_self(window, cx);
        cx.notify();
    }

    fn move_tab(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.file_count();
        if count == 0 {
            return;
        }
        let current = self.tab_cursor.min(count - 1) as isize;
        self.tab_cursor = (current + delta).clamp(0, count as isize - 1) as usize;
        cx.notify();
    }

    fn activate_tab(
        &mut self,
        index: usize,
        keep_zone: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.file_count() {
            return;
        }
        self.file_ix = Some(index);
        self.tab_cursor = index;
        self.clear_compare();
        if !keep_zone {
            self.zone = Zone::Keys;
        }
        self.load_selected_file(cx);
        self.focus_self(window, cx);
        cx.notify();
    }

    fn toggle_compare_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.file_count() {
            return;
        }
        self.tab_cursor = index;
        if self.diff_open {
            let left = self.diff_left;
            let right = self.diff_right;
            if Some(index) == left || Some(index) == right {
                let keep = if left == Some(index) { right } else { left };
                self.clear_compare();
                if let Some(keep) = keep {
                    self.file_ix = Some(keep);
                    self.tab_cursor = keep;
                    self.load_selected_file(cx);
                }
                cx.notify();
                return;
            }
            self.diff_right = Some(index);
            self.load_diff(cx);
            cx.notify();
            return;
        }
        let Some(current) = self.file_ix else {
            self.file_ix = Some(index);
            self.load_selected_file(cx);
            cx.notify();
            return;
        };
        if current == index {
            cx.notify();
            return;
        }
        self.diff_left = Some(current);
        self.diff_right = Some(index);
        self.diff_open = true;
        self.load_diff(cx);
        cx.notify();
    }

    fn clear_compare(&mut self) {
        self.diff_open = false;
        self.diff_cursor = 0;
        self.diff_left = None;
        self.diff_right = None;
        self.diff_left_doc = None;
        self.diff_right_doc = None;
        self.diff_report = None;
    }

    fn compare_with_example(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.project_ix.and_then(|ix| self.projects.get(ix)) else {
            return;
        };
        let Some(example_ix) = project
            .files
            .iter()
            .position(|file| is_example_filename(&file.name))
        else {
            self.toggle_diff(cx);
            return;
        };
        let Some(current) = self.file_ix else {
            return;
        };
        if example_ix == current {
            self.toggle_diff(cx);
            return;
        }
        self.diff_left = Some(example_ix);
        self.diff_right = Some(current);
        self.diff_cursor = 0;
        self.diff_open = true;
        self.zone = Zone::Keys;
        self.load_diff(cx);
        cx.notify();
    }

    fn filter_focused(&self, which: Zone, window: &Window, cx: &App) -> bool {
        let handle = match which {
            Zone::Keys => self.key_filter.focus_handle(cx),
            _ => self.project_filter.focus_handle(cx),
        };
        handle.is_focused(window)
    }

    fn focus_keys_zone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.is_none() {
            if let Some(index) = self.project_ix {
                self.remember_project(index, cx);
                self.select_project(index, true, window, cx);
            }
            return;
        }
        self.zone = Zone::Keys;
        self.focus_self(window, cx);
        cx.notify();
    }

    fn toggle_favorite_selected(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self
            .project_ix
            .and_then(|index| self.projects.get(index))
            .map(|project| project.root.clone())
        else {
            self.status = "Select a project first.".into();
            cx.notify();
            return;
        };
        let pinned = self.config.toggle_favorite(&root);
        self.persist_config(cx);
        self.status = if pinned {
            "Pinned.".into()
        } else {
            "Unpinned.".into()
        };
        self.scroll_project_into_view();
        cx.notify();
    }

    fn open_filtered_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.marked_project(true) else {
            return;
        };
        self.remember_project(index, cx);
        self.select_project(index, true, window, cx);
    }

    fn leave_search(&mut self, which: Zone, window: &mut Window, cx: &mut Context<Self>) {
        match which {
            Zone::Keys => {
                self.zone = Zone::Keys;
                let len = self.visible_entry_nodes().len();
                let visible: Vec<usize> = (0..len).collect();
                let selected = (self.cursor < len).then_some(self.cursor);
                if let Some(index) = search_landing(selected, &visible) {
                    self.cursor = index;
                    if let Some(&item_ix) = self.visible_layout().entry_positions.get(index) {
                        self.key_scroll.scroll_to_item(item_ix);
                    }
                }
                self.focus_self(window, cx);
                cx.notify();
            }
            Zone::Projects | Zone::Tabs => {
                self.zone = Zone::Projects;
                let marked = self.marked_project(true);
                self.project_mark = marked.filter(|index| self.project_ix != Some(*index));
                if let Some(index) = marked {
                    self.scroll_project_index(index);
                }
                self.focus_self(window, cx);
                cx.notify();
            }
        }
    }

    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.overlay, Overlay::None) || self.editor.is_some() {
            return;
        }
        self.project_mark = None;
        let handle = if self.zone == Zone::Projects {
            self.project_filter.focus_handle(cx)
        } else {
            self.key_filter.focus_handle(cx)
        };
        window.focus(&handle, cx);
        cx.notify();
    }

    fn clear_key_filter(&mut self, keep_focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.key_query.clear();
        self.key_filter
            .update(cx, |input, cx| input.set_value("", window, cx));
        if keep_focus {
            let handle = self.key_filter.focus_handle(cx);
            window.focus(&handle, cx);
        } else {
            self.zone = Zone::Keys;
            self.focus_self(window, cx);
        }
        cx.notify();
    }

    fn clear_project_filter(
        &mut self,
        keep_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filter_query.clear();
        self.project_filter
            .update(cx, |input, cx| input.set_value("", window, cx));
        if keep_focus {
            let handle = self.project_filter.focus_handle(cx);
            window.focus(&handle, cx);
        } else {
            self.zone = Zone::Projects;
            self.focus_self(window, cx);
            self.scroll_project_into_view();
        }
        cx.notify();
    }

    fn ensure_open(&mut self, cx: &mut Context<Self>) {
        if self.open.is_some()
            || self.io_running
            || self
                .io_queue
                .iter()
                .any(|job| matches!(job, IoJob::Load { .. }))
        {
            return;
        }
        let Some(index) = self.project_ix else {
            return;
        };
        if self.file_ix.is_none() {
            self.file_ix = default_file_index(&self.projects[index]);
        }
        self.load_selected_file(cx);
    }

    fn select_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.activate_tab(index, true, window, cx);
    }

    fn load_selected_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.current_path() else {
            self.open = None;
            self.text_session_before = None;
            self.text_editor_needs_sync = true;
            return;
        };
        self.commit_cell_if_open(cx);
        if self.open.as_ref().is_some_and(|open| open.path == path) {
            return;
        }
        self.finalize_text_session();
        self.text_save_gen = self.text_save_gen.wrapping_add(1);
        self.cell_save_gen = self.cell_save_gen.wrapping_add(1);
        if let Some(open) = self.open.as_ref() {
            if open.is_dirty() {
                self.queue_save(cx);
            }
        }
        self.io_epoch = self.io_epoch.wrapping_add(1);
        let epoch = self.io_epoch;
        self.status = "Opening…".into();
        self.io_queue.push_back(IoJob::Load { epoch, path });
        self.pump_io(cx);
    }

    fn enqueue_save(&mut self, cx: &mut Context<Self>) {
        self.queue_save(cx);
    }

    /// Write the open document. A disk-change notice does not block it:
    /// an edit the user made is written, and a failure stays on screen.
    fn queue_save(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &self.open else {
            return;
        };
        if !open.is_dirty() {
            if self.save_phase == SavePhase::Failed && self.save_failed.is_none() {
                self.save_phase = SavePhase::Idle;
            }
            return;
        }
        let text = open.document.serialize();
        let path = open.path.clone();
        self.queue_save_text(path, text, cx);
    }

    fn queue_save_text(&mut self, path: PathBuf, text: String, cx: &mut Context<Self>) {
        self.save_seq = self.save_seq.wrapping_add(1);
        self.save_phase = SavePhase::Saving;
        self.io_queue.push_back(IoJob::Save {
            seq: self.save_seq,
            path,
            text,
        });
        self.pump_io(cx);
        cx.notify();
    }

    fn pump_io(&mut self, cx: &mut Context<Self>) {
        if self.io_running {
            return;
        }
        let Some(job) = self.io_queue.pop_front() else {
            return;
        };
        self.save_active = matches!(job, IoJob::Save { .. });
        self.io_running = true;
        cx.notify();
        cx.spawn(async move |this, async_cx| {
            let done = async_cx
                .background_executor()
                .spawn(async move { run_io(job) })
                .await;
            let _ = this.update(async_cx, |view, cx| {
                view.io_running = false;
                view.apply_io(done, cx);
                view.pump_io(cx);
            });
        })
        .detach();
    }

    fn apply_io(&mut self, done: IoDone, cx: &mut Context<Self>) {
        self.save_active = false;
        match done {
            IoDone::Saved {
                seq,
                path,
                text,
                result,
            } => match result {
                Ok(mtime) => {
                    let same = self
                        .open
                        .as_ref()
                        .is_some_and(|open| open.path == path && open.document.serialize() == text);
                    if same {
                        if let Some(open) = self.open.as_mut() {
                            open.note_saved(text.clone(), mtime);
                        }
                    }
                    if self
                        .save_failed
                        .as_ref()
                        .is_some_and(|failed| failed.path == path)
                    {
                        self.save_failed = None;
                    }
                    self.settle_after_save(seq, cx);
                }
                Err(_) => {
                    let newer_queued = self.io_queue.iter().any(
                        |job| matches!(job, IoJob::Save { path: queued, .. } if queued == &path),
                    );
                    if !newer_queued {
                        self.save_failed = Some(FailedSave { path, text });
                        self.save_phase = SavePhase::Failed;
                        self.saved_flash = self.saved_flash.wrapping_add(1);
                        self.io_queue
                            .retain(|job| !matches!(job, IoJob::Load { .. } | IoJob::Quit));
                    }
                }
            },
            IoDone::Loaded {
                epoch,
                path,
                result,
            } => {
                if self.io_epoch != epoch {
                    return;
                }
                match result {
                    Ok(open) => {
                        self.open = Some(open);
                        self.cursor = 0;
                        self.revealed.clear();
                        self.close_editor();
                        self.text_session_before = None;
                        self.text_editor_needs_sync = true;
                        if self.diff_open {
                            self.load_diff(cx);
                        }
                        if let Some(key) = self.pending_key.take() {
                            self.jump_to_key(&key);
                            self.status = format!("Jumped to {key}");
                        } else if self.status == "Opening…" || self.status == "Saving…" {
                            self.status.clear();
                        }
                        self.hold_open_focus(cx);
                        let _ = path;
                    }
                    Err(err) => {
                        self.open = None;
                        self.text_session_before = None;
                        self.text_editor_needs_sync = true;
                        self.status = format!("Could not open file: {err}");
                    }
                }
            }
            IoDone::Reloaded { epoch, result } => {
                if self.io_epoch != epoch {
                    return;
                }
                match result {
                    Ok(fresh) => {
                        if let Some(open) = self.open.as_mut() {
                            open.replace_from_disk(fresh);
                        }
                        if !self.reload_keep_cursor {
                            self.cursor = 0;
                        }
                        self.reload_keep_cursor = false;
                        let len = self.visible_entry_nodes().len();
                        if len == 0 {
                            self.cursor = 0;
                        } else if self.cursor >= len {
                            self.cursor = len - 1;
                        }
                        self.text_session_before = None;
                        self.text_editor_needs_sync = true;
                        self.sync_keys();
                        self.diff_stale = true;
                        self.status = "Reloaded from disk.".into();
                    }
                    Err(err) => self.status = format!("Reload failed: {err}"),
                }
            }
            IoDone::Deleted { name, result } => match result {
                Ok(()) => {
                    self.status = format!("Deleted {name}.");
                    self.begin_scan(true, cx);
                }
                Err(_) => {
                    self.status = "Couldn't delete that file.".into();
                    self.begin_scan(true, cx);
                }
            },
            IoDone::Quit => cx.quit(),
        }
        cx.notify();
    }

    fn schedule_disk_check(&mut self, cx: &mut Context<Self>) {
        if self.disk_check || self.last_disk.elapsed() < Duration::from_millis(300) {
            return;
        }
        if self.save_active
            || self
                .io_queue
                .iter()
                .any(|job| matches!(job, IoJob::Save { .. }))
        {
            return;
        }
        let Some(open) = &self.open else {
            return;
        };
        self.last_disk = Instant::now();
        self.disk_check = true;
        let (path, known, ignored, document) = open.disk_parts();
        let epoch = self.io_epoch;
        cx.spawn(async move |this, async_cx| {
            let notice = async_cx
                .background_executor()
                .spawn(async move {
                    envy_core::inspect_disk(&path, known, ignored, &document.serialize())
                })
                .await;
            let _ = this.update(async_cx, |view, cx| {
                view.disk_check = false;
                if view.io_epoch != epoch {
                    return;
                }
                let Ok(notice) = notice else {
                    return;
                };
                let tell = view
                    .open
                    .as_mut()
                    .is_some_and(|open| open.apply_disk_notice(notice));
                if tell {
                    let mid_edit = view.editor.is_some()
                        || view.editor_view == EditorView::Text
                        || view.open.as_ref().is_some_and(|open| open.is_dirty());
                    if mid_edit {
                        view.status = "Changed on disk. R reloads, Esc keeps yours.".into();
                    } else {
                        view.begin_reload(true, cx);
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn choose_diff_sides(&mut self) {
        let Some(project) = self.project_ix.and_then(|ix| self.projects.get(ix)) else {
            self.diff_left = None;
            self.diff_right = None;
            return;
        };
        if project.files.is_empty() {
            self.diff_left = None;
            self.diff_right = None;
            return;
        }
        let last = project.files.len() - 1;
        let current = self.file_ix.unwrap_or(0).min(last);
        if self.diff_right.is_none_or(|index| index > last) {
            self.diff_right = Some(current);
        }
        let right = self.diff_right.unwrap_or(current);
        let left_bad = self
            .diff_left
            .is_none_or(|index| index > last || (index == right && project.files.len() > 1));
        if left_bad {
            self.diff_left = project
                .files
                .iter()
                .enumerate()
                .find(|(index, file)| *index != right && is_example_filename(&file.name))
                .map(|(index, _)| index)
                .or_else(|| {
                    project
                        .files
                        .iter()
                        .enumerate()
                        .find(|(index, _)| *index != right)
                        .map(|(index, _)| index)
                })
                .or(Some(right));
        }
    }

    fn side_index(&self, side: DiffSide) -> Option<usize> {
        match side {
            DiffSide::Left => self.diff_left,
            DiffSide::Right => self.diff_right,
        }
    }

    fn side_path(&self, side: DiffSide) -> Option<PathBuf> {
        let project = self.projects.get(self.project_ix?)?;
        let index = self.side_index(side)?;
        Some(project.files.get(index)?.path.clone())
    }

    fn side_name(&self, side: DiffSide) -> (String, String) {
        let Some(index) = (match side {
            DiffSide::Left => self.diff_left,
            DiffSide::Right => self.diff_right,
        }) else {
            return ("Choose a file".into(), String::new());
        };
        self.current_tab_labels()
            .get(index)
            .map(|label| (label.name.clone(), label.suffix.clone()))
            .unwrap_or_else(|| ("Choose a file".into(), String::new()))
    }

    fn current_tab_labels(&self) -> Vec<envy_core::TabLabel> {
        let Some(project) = self.project_ix.and_then(|ix| self.projects.get(ix)) else {
            return Vec::new();
        };
        let relative: Vec<String> = project
            .files
            .iter()
            .map(|file| relative_slash_path(&project.root, &file.path))
            .collect();
        let borrowed: Vec<&str> = relative.iter().map(String::as_str).collect();
        tab_labels(&borrowed)
    }

    fn doc_if_open(&self, path: &Path) -> Option<EnvDocument> {
        self.open
            .as_ref()
            .filter(|open| open.path == path)
            .map(|open| open.document.clone())
    }

    fn load_diff(&mut self, cx: &mut Context<Self>) {
        self.choose_diff_sides();
        self.diff_epoch = self.diff_epoch.wrapping_add(1);
        let epoch = self.diff_epoch;
        self.diff_report = None;
        let Some(left_path) = self.side_path(DiffSide::Left) else {
            self.diff_left_doc = None;
            self.diff_right_doc = None;
            return;
        };
        let Some(right_path) = self.side_path(DiffSide::Right) else {
            self.diff_left_doc = None;
            self.diff_right_doc = None;
            return;
        };
        let left_mem = self.doc_if_open(&left_path);
        let right_mem = self.doc_if_open(&right_path);
        cx.spawn(async move |this, async_cx| {
            let loaded = async_cx
                .background_executor()
                .spawn(async move {
                    let left = match left_mem {
                        Some(doc) => doc,
                        None => EnvDocument::parse(&std::fs::read_to_string(&left_path).ok()?),
                    };
                    let right = match right_mem {
                        Some(doc) => doc,
                        None => EnvDocument::parse(&std::fs::read_to_string(&right_path).ok()?),
                    };
                    let report = diff::diff(&left, &right);
                    Some((left, right, report))
                })
                .await;
            let _ = this.update(async_cx, |view, cx| {
                if view.diff_epoch != epoch {
                    return;
                }
                match loaded {
                    Some((left, right, report)) => {
                        view.diff_left_doc = Some(left);
                        view.diff_right_doc = Some(right);
                        view.diff_report = Some(report);
                    }
                    None => {
                        view.diff_left_doc = None;
                        view.diff_right_doc = None;
                        view.diff_report = None;
                        view.status = "Couldn't read one of those files.".into();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn refresh_open_side(&mut self) {
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let path = open.path.clone();
        let doc = open.document.clone();
        if self.side_path(DiffSide::Left).as_ref() == Some(&path) {
            self.diff_left_doc = Some(doc.clone());
        }
        if self.side_path(DiffSide::Right).as_ref() == Some(&path) {
            self.diff_right_doc = Some(doc);
        }
    }

    fn recompute_diff(&mut self, cx: &mut Context<Self>) {
        if !self.diff_open {
            return;
        }
        self.refresh_open_side();
        let Some(left) = self.diff_left_doc.clone() else {
            self.load_diff(cx);
            return;
        };
        let Some(right) = self.diff_right_doc.clone() else {
            self.load_diff(cx);
            return;
        };
        self.diff_epoch = self.diff_epoch.wrapping_add(1);
        let epoch = self.diff_epoch;
        cx.spawn(async move |this, async_cx| {
            let report = async_cx
                .background_executor()
                .spawn(async move { diff::diff(&left, &right) })
                .await;
            let _ = this.update(async_cx, |view, cx| {
                if view.diff_epoch != epoch {
                    return;
                }
                view.diff_report = Some(report);
                view.diff_stale = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn focus_self(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }

    /// Put the caret in Find a project. A restored file must not take it,
    /// and neither may a search field the user is already typing in.
    fn hold_open_focus(&mut self, cx: &mut Context<Self>) {
        if !self.focus_project_on_open {
            return;
        }
        let project = self.project_filter.focus_handle(cx);
        let keys = self.key_filter.focus_handle(cx);
        let mut leave_keys = false;
        self.window
            .update(cx, |_, window, cx| {
                if keys.is_focused(window) {
                    leave_keys = true;
                    return;
                }
                window.focus(&project, cx);
            })
            .ok();
        if leave_keys {
            self.focus_project_on_open = false;
            return;
        }
        self.zone = Zone::Projects;
    }

    fn close_editor(&mut self) {
        if self.editor.is_some() {
            self.edit_gen = self.edit_gen.wrapping_add(1);
        }
        self.editor = None;
        self.edit_subs.clear();
    }

    fn cell_fields_focused(&self, window: &Window, cx: &App) -> bool {
        let Some(editor) = &self.editor else {
            return false;
        };
        editor.value.focus_handle(cx).is_focused(window)
            || editor
                .key
                .as_ref()
                .is_some_and(|key| key.focus_handle(cx).is_focused(window))
    }

    fn on_cell_blur(&mut self, gen: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit_gen != gen || self.editor.is_none() {
            return;
        }
        if self.cell_fields_focused(window, cx) {
            return;
        }
        self.commit_editor(None, false, window, cx);
    }

    fn on_cell_change(&mut self, cx: &mut Context<Self>) {
        if self.sync_cell_preview(false, cx) {
            self.touch_edit();
            self.schedule_cell_save(cx);
            cx.notify();
        }
    }

    fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        self.focus_self(window, cx);
        cx.notify();
    }

    /// Save if needed, then quit. Returns false while a save is still running
    /// so the window stays up until the write finishes.
    pub fn close_requested(&mut self, cx: &mut Context<Self>) -> bool {
        self.commit_cell_if_open(cx);
        self.finalize_text_session();
        self.text_save_gen = self.text_save_gen.wrapping_add(1);
        self.cell_save_gen = self.cell_save_gen.wrapping_add(1);
        if self.open.as_ref().is_some_and(|open| open.is_dirty()) {
            self.queue_save(cx);
            self.io_queue.push_back(IoJob::Quit);
            self.pump_io(cx);
            false
        } else if self.save_active
            || self
                .io_queue
                .iter()
                .any(|job| matches!(job, IoJob::Save { .. }))
        {
            self.io_queue.push_back(IoJob::Quit);
            self.pump_io(cx);
            false
        } else {
            cx.quit();
            true
        }
    }

    fn save_open(&mut self, cx: &mut Context<Self>) {
        self.commit_cell_if_open(cx);
        self.finalize_text_session();
        self.text_save_gen = self.text_save_gen.wrapping_add(1);
        self.cell_save_gen = self.cell_save_gen.wrapping_add(1);
        self.queue_save(cx);
        cx.notify();
    }

    fn mark_edited(&mut self, cx: &mut Context<Self>) {
        self.touch_edit();
        if self.editor_view == EditorView::Text {
            self.schedule_text_save(cx);
        } else {
            self.enqueue_save(cx);
        }
    }

    fn touch_edit(&mut self) {
        self.io_epoch = self.io_epoch.wrapping_add(1);
        self.io_queue
            .retain(|job| !matches!(job, IoJob::Load { .. }));
        self.diff_stale = true;
        if self.editor_view == EditorView::Table {
            self.text_editor_needs_sync = true;
        }
        self.sync_keys();
        if self.save_phase == SavePhase::Saved {
            self.save_phase = SavePhase::Idle;
            self.saved_flash = self.saved_flash.wrapping_add(1);
        }
    }

    fn schedule_text_save(&mut self, cx: &mut Context<Self>) {
        self.text_save_gen = self.text_save_gen.wrapping_add(1);
        let gen = self.text_save_gen;
        cx.spawn(async move |this, async_cx| {
            async_cx.background_executor().timer(SAVE_PAUSE).await;
            let _ = this.update(async_cx, |view, cx| {
                if view.text_save_gen != gen || view.editor_view != EditorView::Text {
                    return;
                }
                view.enqueue_save(cx);
            });
        })
        .detach();
    }

    fn schedule_cell_save(&mut self, cx: &mut Context<Self>) {
        self.cell_save_gen = self.cell_save_gen.wrapping_add(1);
        let gen = self.cell_save_gen;
        cx.spawn(async move |this, async_cx| {
            async_cx.background_executor().timer(SAVE_PAUSE).await;
            let _ = this.update(async_cx, |view, cx| {
                if view.cell_save_gen != gen || view.editor.is_none() {
                    return;
                }
                view.queue_save(cx);
            });
        })
        .detach();
    }

    fn flush_save(&mut self, cx: &mut Context<Self>) {
        self.text_save_gen = self.text_save_gen.wrapping_add(1);
        self.cell_save_gen = self.cell_save_gen.wrapping_add(1);
        self.queue_save(cx);
    }

    fn show_saved(&mut self, cx: &mut Context<Self>) {
        self.save_phase = SavePhase::Saved;
        self.saved_flash = self.saved_flash.wrapping_add(1);
        let token = self.saved_flash;
        cx.spawn(async move |this, async_cx| {
            async_cx
                .background_executor()
                .timer(SAVED_FLASH_AFTER)
                .await;
            let _ = this.update(async_cx, |view, cx| {
                if view.saved_flash == token && view.save_phase == SavePhase::Saved {
                    view.save_phase = SavePhase::Idle;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn settle_after_save(&mut self, seq: u64, cx: &mut Context<Self>) {
        if seq != self.save_seq {
            return;
        }
        if self.open.as_ref().is_some_and(|open| open.is_dirty()) {
            let path = self
                .open
                .as_ref()
                .map(|open| open.path.clone())
                .unwrap_or_default();
            let queued = self
                .io_queue
                .iter()
                .any(|job| matches!(job, IoJob::Save { path: queued, .. } if queued == &path));
            if !queued {
                self.queue_save(cx);
            }
            return;
        }
        if self.save_failed.is_none() {
            self.show_saved(cx);
        }
    }

    fn retry_save(&mut self, cx: &mut Context<Self>) {
        if self.open.as_ref().is_some_and(|open| open.is_dirty()) {
            self.queue_save(cx);
            return;
        }
        let Some(failed) = self.save_failed.clone() else {
            self.save_phase = SavePhase::Idle;
            cx.notify();
            return;
        };
        self.queue_save_text(failed.path, failed.text, cx);
    }

    fn finalize_text_session(&mut self) {
        let Some(before) = self.text_session_before.take() else {
            return;
        };
        if let Some(open) = self.open.as_mut() {
            let text = open.document.serialize();
            open.commit_text_snapshot(before, &text);
        }
    }

    fn sync_text_editor_if_needed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.text_editor_needs_sync || self.editor_view != EditorView::Text {
            return;
        }
        let text = self
            .open
            .as_ref()
            .map(|open| open.document.serialize())
            .unwrap_or_default();
        self.text_editor_needs_sync = false;
        self.text_editor_syncing = true;
        self.text_editor
            .update(cx, |editor, cx| editor.set_value(text, window, cx));
        self.text_editor_syncing = false;
    }

    fn toggle_editor_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.is_none() {
            self.status = "Open a file first.".into();
            cx.notify();
            return;
        }
        self.commit_cell_if_open(cx);
        if self.diff_open {
            self.clear_compare();
        }
        match self.editor_view {
            EditorView::Table => {
                self.editor_view = EditorView::Text;
                self.text_session_before = self.open.as_ref().map(|open| open.document.clone());
                self.text_editor_needs_sync = true;
                self.sync_text_editor_if_needed(window, cx);
                let handle = self.text_editor.focus_handle(cx);
                window.focus(&handle, cx);
            }
            EditorView::Text => {
                self.finalize_text_session();
                self.text_save_gen = self.text_save_gen.wrapping_add(1);
                self.enqueue_save(cx);
                self.editor_view = EditorView::Table;
                self.zone = Zone::Keys;
                let len = self.visible_entry_nodes().len();
                if self.cursor >= len {
                    self.cursor = len.saturating_sub(1);
                }
                self.focus_self(window, cx);
            }
        }
        self.config.settings.editor_view = self.editor_view;
        self.persist_config(cx);
        cx.notify();
    }

    fn compute_suggestions(&self, query: &str) -> Vec<KeySuggestion> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        let Some(open) = &self.open else {
            return Vec::new();
        };
        let rename_node = self
            .editor
            .as_ref()
            .filter(|editor| matches!(editor.kind, EditKind::Rename))
            .and_then(|editor| editor.node);
        let current_keys: Vec<String> = open
            .document
            .entries()
            .filter(|(idx, _)| Some(*idx) != rename_node)
            .map(|(_, entry)| entry.key.clone())
            .collect();

        let results = suggest_keys(
            &self.projects,
            self.project_ix,
            Some(open.path.as_path()),
            &current_keys,
            query,
            5,
        );
        if results.len() == 1 && results[0].key.eq_ignore_ascii_case(query) {
            return Vec::new();
        }
        results
    }

    fn refresh_editor_suggestions(&mut self, query: &str) {
        let suggestions = self.compute_suggestions(query);
        if let Some(editor) = self.editor.as_mut() {
            editor.suggestions_open = !suggestions.is_empty();
            editor.suggestions = suggestions;
        }
    }

    fn key_editor_focused(&self, window: &Window, cx: &App) -> bool {
        self.editor
            .as_ref()
            .and_then(|editor| editor.key.as_ref())
            .is_some_and(|input| input.focus_handle(cx).is_focused(window))
    }

    fn accept_suggestion(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((key, tag, key_input)) = self.editor.as_ref().and_then(|ed| {
            let suggestion = ed.suggestions.get(index)?;
            Some((suggestion.key.clone(), ed.kind_tag(), ed.key.clone()?))
        }) else {
            return false;
        };
        key_input.update(cx, |input, cx| input.set_value(key.clone(), window, cx));
        if let Some(editor) = self.editor.as_mut() {
            editor.suggestions_open = false;
        }
        match tag {
            EditTag::New => {
                self.focus_value_editor(window, cx);
                cx.notify();
            }
            EditTag::Rename => {
                self.commit_editor_key(key, window, cx);
            }
            EditTag::Value => {}
        }
        true
    }

    fn sync_keys(&mut self) {
        let Some(open) = &self.open else {
            return;
        };
        let keys = open.document.key_names();
        let active_keys = open.document.active_key_names();
        let path = open.path.clone();
        let Some(project) = self.project_ix.and_then(|ix| self.projects.get_mut(ix)) else {
            return;
        };
        if let Some(file) = project.files.iter_mut().find(|file| file.path == path) {
            file.keys = keys;
            file.active_keys = active_keys;
        }
        recompute_missing(project);
    }

    fn follow_link_cx(&mut self, link: Deeplink, cx: &mut Context<Self>) {
        self.follow_link_inner(&link, cx);
        cx.notify();
    }

    fn follow_link_inner(&mut self, link: &Deeplink, cx: &mut Context<Self>) {
        let Some(project_ix) = resolve_project(&self.projects, &self.config, &link.repo) else {
            self.status = format!("No project named {}", link.repo);
            return;
        };
        self.project_ix = Some(project_ix);
        if let Some(file) = &link.file {
            match resolve_file(&self.projects[project_ix], file) {
                Some(index) => self.file_ix = Some(index),
                None => {
                    self.file_ix = default_file_index(&self.projects[project_ix]);
                    self.status = format!("No file {file} in {}", link.repo);
                }
            }
        } else {
            self.file_ix = default_file_index(&self.projects[project_ix]);
        }
        self.zone = Zone::Keys;
        self.pending_key = link.key.clone();
        self.load_selected_file(cx);
    }

    fn jump_to_key(&mut self, key: &str) {
        let layout = self.visible_layout();
        let Some(open) = &self.open else {
            return;
        };
        if let Some(position) =
            layout
                .entry_nodes
                .iter()
                .position(|node| match open.document.nodes.get(*node) {
                    Some(Node::Entry(entry)) => entry.key == key,
                    _ => false,
                })
        {
            self.cursor = position;
            if let Some(&item_ix) = layout.entry_positions.get(position) {
                self.key_scroll.scroll_to_item(item_ix);
            }
            self.zone = Zone::Keys;
        }
    }

    fn move_delta(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.tab_menu {
            menu.cursor = step_index(menu.cursor, TAB_MENU_LEN, delta);
            cx.notify();
            return;
        }
        if self.step_create_source(delta, window, cx) {
            return;
        }
        if !matches!(self.overlay, Overlay::None) {
            return;
        }
        match self.zone {
            Zone::Projects => {
                let visible = self.visible_project_indexes();
                if visible.is_empty() {
                    if delta < 0 {
                        self.focus_search(window, cx);
                    }
                    return;
                }
                let anchor = self.marked_project(false);
                let current = anchor
                    .and_then(|index| visible.iter().position(|item| *item == index))
                    .map(|position| position as isize)
                    .unwrap_or(-1);
                if delta < 0 && current <= 0 {
                    self.focus_search(window, cx);
                    return;
                }
                let next_pos = (current + delta).clamp(0, visible.len() as isize - 1) as usize;
                let next = visible[next_pos];
                if self.project_mark.is_some() {
                    self.project_mark = Some(next);
                    self.scroll_project_index(next);
                    self.focus_self(window, cx);
                    cx.notify();
                } else {
                    self.select_project(next, false, window, cx);
                }
            }
            Zone::Tabs => {
                if delta > 0 {
                    self.focus_keys_zone(window, cx);
                } else {
                    self.move_tab(-1, cx);
                }
            }
            Zone::Keys if self.diff_open => {
                let len = self
                    .diff_report
                    .as_ref()
                    .map(|report| report.rows.len() as isize)
                    .unwrap_or(0);
                if len == 0 {
                    return;
                }
                let next = (self.diff_cursor as isize + delta).clamp(0, len - 1) as usize;
                self.diff_cursor = next;
                self.diff_scroll.scroll_to_item(next);
                cx.notify();
            }
            Zone::Keys => {
                let layout = self.visible_layout();
                let len = layout.entry_nodes.len() as isize;
                if len == 0 {
                    if delta < 0 {
                        self.focus_search(window, cx);
                    }
                    return;
                }
                if delta < 0 && self.cursor == 0 {
                    self.focus_search(window, cx);
                    return;
                }
                let next = (self.cursor as isize + delta).clamp(0, len - 1) as usize;
                self.cursor = next;
                if let Some(&item_ix) = layout.entry_positions.get(next) {
                    self.key_scroll.scroll_to_item(item_ix);
                }
                cx.notify();
            }
        }
    }

    fn move_file(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.nudge_copy_mode(delta < 0, cx) || self.tab_menu.is_some() {
            return;
        }
        let Some(project) = self.project_ix.and_then(|ix| self.projects.get(ix)) else {
            return;
        };
        if project.files.is_empty() {
            return;
        }
        let current = self.file_ix.unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, project.files.len() as isize - 1) as usize;
        self.select_file(next, window, cx);
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.activate_tab_menu(window, cx) {
            return;
        }
        if let Overlay::DeleteFile { file_ix, .. } = &self.overlay {
            let file_ix = *file_ix;
            self.delete_tab(file_ix, window, cx);
            return;
        }
        match &self.overlay {
            Overlay::Paste(_) => {
                self.apply_overlay_paste(window, cx);
                return;
            }
            Overlay::Shortcuts | Overlay::Unreadable => {
                self.close_overlay(window, cx);
                return;
            }
            Overlay::AddFolder => return,
            Overlay::Settings => return,
            Overlay::DeleteFile { .. } => return,
            Overlay::None => {}
        }
        if self.create_source_focused() {
            self.commit_file_prompt(window, cx);
            return;
        }
        match self.zone {
            Zone::Projects => {
                if let Some(index) = self.marked_project(false) {
                    self.remember_project(index, cx);
                    self.select_project(index, true, window, cx);
                }
            }
            Zone::Tabs => self.activate_tab(self.tab_cursor, false, window, cx),
            Zone::Keys => self.begin_value_edit(window, cx),
        }
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.overlay, Overlay::None) {
            self.close_overlay(window, cx);
            return;
        }
        if self.tab_menu.is_some() {
            self.tab_menu = None;
            self.focus_self(window, cx);
            cx.notify();
            return;
        }
        if !matches!(self.file_prompt, FilePrompt::None) {
            self.close_file_prompt();
            self.focus_self(window, cx);
            cx.notify();
            return;
        }
        if self.editor.is_some() && self.zone == Zone::Keys {
            self.apply_cell_motion(CellMotion::Cancel, window, cx);
            return;
        }
        if self.editor.is_some() {
            self.commit_cell_if_open(cx);
            self.focus_self(window, cx);
            cx.notify();
            return;
        }
        let key_typed = self.key_filter.read(cx).value().to_string();
        let key_focused = self.key_filter.focus_handle(cx).is_focused(window);
        let typed = self.project_filter.read(cx).value().to_string();
        let filter_focused = self.project_filter.focus_handle(cx).is_focused(window);
        if key_focused {
            if key_typed.is_empty() && self.key_query.is_empty() {
                self.zone = Zone::Keys;
                self.focus_self(window, cx);
                cx.notify();
            } else {
                self.clear_key_filter(true, window, cx);
            }
            return;
        }
        if filter_focused {
            if typed.is_empty() && self.filter_query.is_empty() {
                self.zone = Zone::Projects;
                self.focus_self(window, cx);
                cx.notify();
            } else {
                self.clear_project_filter(true, window, cx);
            }
            return;
        }
        if self.open.as_ref().is_some_and(|open| open.external_change) {
            if let Some(open) = self.open.as_mut() {
                open.dismiss_external();
            }
            self.status = "Keeping your copy.".into();
            self.queue_save(cx);
            cx.notify();
            return;
        }
        if self.diff_open {
            self.clear_compare();
            cx.notify();
            return;
        }
        self.status.clear();
        cx.notify();
    }

    fn begin_value_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.selected_node() else {
            self.status = "No key selected.".into();
            cx.notify();
            return;
        };
        let value = match self
            .open
            .as_ref()
            .and_then(|open| open.document.nodes.get(node))
        {
            Some(Node::Entry(entry)) => entry.value.clone(),
            _ => return,
        };
        self.open_editor(EditKind::Value, Some(node), None, Some(value), window, cx);
    }

    fn begin_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.list_keys_blocked() {
            return;
        }
        let Some(node) = self.selected_node() else {
            return;
        };
        let key = match self
            .open
            .as_ref()
            .and_then(|open| open.document.nodes.get(node))
        {
            Some(Node::Entry(entry)) => entry.key.clone(),
            _ => return,
        };
        self.open_editor(EditKind::Rename, Some(node), Some(key), None, window, cx);
    }

    fn begin_new_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.list_keys_blocked() {
            return;
        }
        if self.filter_focused(Zone::Projects, window, cx)
            || self.filter_focused(Zone::Keys, window, cx)
        {
            return;
        }
        if self.open.is_none() {
            self.status = "Open a file first.".into();
            cx.notify();
            return;
        }
        if self.editor_view == EditorView::Text {
            self.toggle_editor_view(window, cx);
        }
        self.overlay = Overlay::None;
        self.open_editor(
            EditKind::New,
            None,
            Some(String::new()),
            Some(String::new()),
            window,
            cx,
        );
        self.key_scroll.scroll_to_bottom();
    }

    fn editing_cell(&self) -> Option<EditCell> {
        match self.editor.as_ref().map(|editor| editor.kind_tag()) {
            Some(EditTag::Value) => Some(EditCell::Value),
            Some(EditTag::Rename) => Some(EditCell::Key),
            Some(EditTag::New) | None => None,
        }
    }

    fn apply_cell_motion(
        &mut self,
        motion: CellMotion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .editor
            .as_ref()
            .is_some_and(|editor| matches!(editor.kind, EditKind::New))
        {
            self.apply_new_row_motion(motion, window, cx);
            return;
        }
        let Some(cell) = self.editing_cell() else {
            return;
        };
        let len = self.visible_entry_nodes().len();
        let route = route_cell_edit(self.cursor, len, cell, motion);
        if route.commit {
            if matches!(cell, EditCell::Key) && !self.editor_key_is_valid(cx) {
                self.status = "Keys start with a letter or underscore.".into();
                self.hold_key_editor(false, window, cx);
                cx.notify();
                return;
            }
            self.commit_editor(None, false, window, cx);
        } else {
            self.finish_cell(true, cx);
        }
        if let Some(row) = route.focus_row {
            self.cursor = row.min(len.saturating_sub(1));
            self.zone = Zone::Keys;
        }
        if let Some(cell) = route.focus_cell {
            match cell {
                EditCell::Key => self.begin_rename(window, cx),
                EditCell::Value => self.begin_value_edit(window, cx),
            }
        } else {
            self.focus_self(window, cx);
            cx.notify();
        }
    }

    fn editor_key_is_valid(&self, cx: &App) -> bool {
        let key = self
            .editor
            .as_ref()
            .and_then(|editor| editor.key.as_ref())
            .map(|input| input.read(cx).value().trim().to_string())
            .unwrap_or_default();
        is_env_key(&key)
    }

    fn apply_new_row_motion(
        &mut self,
        motion: CellMotion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key_focused = self.key_editor_focused(window, cx);
        match motion {
            CellMotion::Cancel => self.cancel_cell_edit(window, cx),
            CellMotion::PrevCell if !key_focused => self.hold_key_editor(true, window, cx),
            CellMotion::PrevCell => self.hold_key_editor(false, window, cx),
            CellMotion::NextCell if key_focused => {
                self.hold_value_editor(true, window, cx);
            }
            CellMotion::NextCell => {
                if !self.commit_new_row(window, cx) {
                    return;
                }
                let len = self.visible_entry_nodes().len();
                if len > 0 {
                    self.cursor = len - 1;
                    self.zone = Zone::Keys;
                    self.begin_value_edit(window, cx);
                } else {
                    self.focus_self(window, cx);
                    cx.notify();
                }
            }
            CellMotion::NextRow => {
                if self.commit_new_row(window, cx) {
                    self.focus_self(window, cx);
                }
            }
        }
    }

    fn commit_new_row(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.editor_key_is_valid(cx) {
            self.status = "Keys start with a letter or underscore.".into();
            self.hold_key_editor(false, window, cx);
            cx.notify();
            return false;
        }
        self.commit_editor(None, false, window, cx);
        true
    }

    fn place_value_caret(&self, select: bool, window: &mut Window, cx: &mut App) {
        let Some(editor) = &self.editor else {
            return;
        };
        let handle = editor.value.focus_handle(cx);
        if select && !handle.is_focused(window) {
            editor
                .value
                .update(cx, |state, cx| state.select_all(window, cx));
        }
        window.focus(&handle, cx);
    }

    fn hold_value_editor(&mut self, select: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.place_value_caret(select, window, cx);
        cx.defer_in(window, move |this, window, cx| {
            this.place_value_caret(select, window, cx);
        });
    }

    fn place_key_caret(&self, select: bool, window: &mut Window, cx: &mut App) {
        let Some(key) = self.editor.as_ref().and_then(|editor| editor.key.as_ref()) else {
            return;
        };
        let handle = key.focus_handle(cx);
        if select && !handle.is_focused(window) {
            key.update(cx, |state, cx| state.select_all(window, cx));
        }
        window.focus(&handle, cx);
    }

    fn hold_key_editor(&mut self, select: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.place_key_caret(select, window, cx);
        cx.defer_in(window, move |this, window, cx| {
            this.place_key_caret(select, window, cx);
        });
    }

    fn edit_entry_at(
        &mut self,
        row_ix: usize,
        target: RowTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zone = Zone::Keys;
        let node = self.visible_entry_nodes().get(row_ix).copied();
        let same = self.editor.as_ref().is_some_and(|editor| {
            editor.node == node
                && matches!(
                    (target, &editor.kind),
                    (RowTarget::KeyName, EditKind::Rename) | (RowTarget::Value, EditKind::Value)
                )
        });
        if same {
            self.cursor = row_ix;
            match target {
                RowTarget::Value => self.hold_value_editor(false, window, cx),
                RowTarget::KeyName => self.hold_key_editor(false, window, cx),
            }
            return;
        }
        self.cursor = row_ix;
        if self.editor.is_some() {
            self.commit_editor(None, false, window, cx);
            self.cursor = row_ix;
        }
        match row_click(target) {
            RowClick::EditKey => self.begin_rename(window, cx),
            RowClick::EditValue => self.begin_value_edit(window, cx),
        }
    }

    fn effective_key_column(&self) -> f32 {
        if let Some(width) = self.key_column_width {
            return clamp_key_column_px(width);
        }
        let labels = self.open.as_ref().map(|open| {
            open.document
                .entries()
                .map(|(_, entry)| {
                    if entry.exported {
                        format!("export {}", entry.key)
                    } else {
                        entry.key.clone()
                    }
                })
                .collect::<Vec<_>>()
        });
        fitted_key_column_px(labels.unwrap_or_default())
    }

    fn begin_resize(&mut self, kind: ResizeKind, x: f32, cx: &mut Context<Self>) {
        let origin_w = match kind {
            ResizeKind::Sidebar => self.sidebar_width,
            ResizeKind::KeyColumn => self.effective_key_column(),
        };
        self.resize_drag = Some(ResizeDrag {
            kind,
            origin_x: x,
            origin_w,
        });
        cx.notify();
    }

    fn update_resize(&mut self, x: f32, cx: &mut Context<Self>) {
        let Some(drag) = &self.resize_drag else {
            return;
        };
        let next = drag.origin_w + (x - drag.origin_x);
        match drag.kind {
            ResizeKind::Sidebar => self.sidebar_width = clamp_sidebar_px(next),
            ResizeKind::KeyColumn => self.key_column_width = Some(clamp_key_column_px(next)),
        }
        cx.notify();
    }

    fn end_resize(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.resize_drag.take() else {
            return;
        };
        match drag.kind {
            ResizeKind::Sidebar => {
                self.config.settings.sidebar_width = Some(self.sidebar_width.round() as u32);
            }
            ResizeKind::KeyColumn => {
                if let Some(width) = self.key_column_width {
                    self.config.settings.key_column_width = Some(width.round() as u32);
                }
            }
        }
        self.persist_config(cx);
        cx.notify();
    }

    fn open_editor(
        &mut self,
        kind: EditKind,
        node: Option<usize>,
        key: Option<String>,
        value: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor.is_some() {
            self.commit_cell_if_open(cx);
        }
        let baseline = self
            .open
            .as_ref()
            .map(|open| open.document.clone())
            .unwrap_or_default();
        let initial_query = key.clone().unwrap_or_default();
        let key_input = key.map(|text| cx.new(|cx| TextInput::new("KEY", cx).with_text(text)));
        let value_text = value.unwrap_or_default();
        let value_input = cx.new(|cx| TextInput::new("value", cx).with_text(value_text));
        if let Some(key_input) = &key_input {
            self.edit_subs.push(cx.subscribe_in(
                key_input,
                window,
                |this, input, event, window, cx| match event {
                    TextInputEvent::Change => {
                        let query = input.read(cx).value().to_string();
                        this.refresh_editor_suggestions(&query);
                        this.on_cell_change(cx);
                        cx.notify();
                    }
                    // The field binding takes Enter before the window does.
                    TextInputEvent::Submit => {
                        this.apply_cell_motion(CellMotion::NextRow, window, cx);
                    }
                    TextInputEvent::Blur => {
                        let gen = this.edit_gen;
                        cx.defer_in(window, move |this, window, cx| {
                            this.on_cell_blur(gen, window, cx);
                        });
                    }
                    TextInputEvent::ExitDown | TextInputEvent::Focus => {}
                },
            ));
        }
        self.edit_subs.push(
            cx.subscribe_in(
                &value_input,
                window,
                |this, _, event, window, cx| match event {
                    TextInputEvent::Submit => {
                        this.apply_cell_motion(CellMotion::NextRow, window, cx);
                    }
                    TextInputEvent::Change => this.on_cell_change(cx),
                    TextInputEvent::Blur => {
                        let gen = this.edit_gen;
                        cx.defer_in(window, move |this, window, cx| {
                            this.on_cell_blur(gen, window, cx);
                        });
                    }
                    TextInputEvent::Focus | TextInputEvent::ExitDown => {}
                },
            ),
        );
        let focus_key = key_input.is_some() && !matches!(kind, EditKind::Value);
        self.editor = Some(Editor {
            key: key_input,
            value: value_input,
            node,
            kind,
            suggestions: Vec::new(),
            suggestions_open: false,
            baseline,
            undo_armed: false,
            preview_node: None,
        });
        if focus_key {
            self.refresh_editor_suggestions(&initial_query);
            self.hold_key_editor(true, window, cx);
        } else {
            self.hold_value_editor(true, window, cx);
        }
        cx.notify();
    }

    fn focus_value_editor(&self, window: &mut Window, cx: &mut App) {
        if let Some(editor) = &self.editor {
            let handle = editor.value.focus_handle(cx);
            window.focus(&handle, cx);
        }
    }

    fn commit_editor_key(&mut self, _key: String, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_editor(None, true, window, cx);
    }

    fn commit_editor(
        &mut self,
        _value_text: Option<String>,
        restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_cell(false, cx);
        if restore_focus {
            self.focus_self(window, cx);
        }
        cx.notify();
    }

    fn commit_cell_if_open(&mut self, cx: &mut Context<Self>) {
        if self.editor.is_none() {
            return;
        }
        self.finish_cell(false, cx);
    }

    fn cancel_cell_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_cell(true, cx);
        self.focus_self(window, cx);
        cx.notify();
    }

    /// Apply the open cell, or put the document back when `cancel` is set,
    /// then write immediately.
    fn finish_cell(&mut self, cancel: bool, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let kind = editor.kind_tag();
        let node = editor.node;
        let key_text = editor
            .key
            .as_ref()
            .map(|input| input.read(cx).value().trim().to_string())
            .unwrap_or_default();
        let baseline = editor.baseline.clone();
        let armed = editor.undo_armed;
        let before = baseline.serialize();
        if cancel {
            self.close_editor();
            if let Some(open) = self.open.as_mut() {
                open.restore_baseline(baseline, armed);
            }
        } else {
            self.sync_cell_preview(true, cx);
            let changed = self
                .open
                .as_ref()
                .is_some_and(|open| open.document.serialize() != before);
            let existed = baseline.contains_key(&key_text);
            let taken = matches!(kind, EditTag::Rename)
                && is_env_key(&key_text)
                && baseline
                    .entries()
                    .any(|(index, entry)| node != Some(index) && entry.key == key_text);
            if taken {
                self.status = format!("{key_text} is already in this file.");
            }
            self.close_editor();
            if changed {
                match kind {
                    EditTag::Rename => {
                        self.jump_to_key(&key_text);
                        self.status = format!("Renamed to {key_text}.");
                    }
                    EditTag::New => {
                        self.jump_to_key(&key_text);
                        self.status = if existed {
                            format!("Updated {key_text}.")
                        } else {
                            format!("Added {key_text}.")
                        };
                    }
                    EditTag::Value => {}
                }
            }
        }
        self.touch_edit();
        self.flush_save(cx);
    }

    /// Fold the open cell into the document. Returns true when the bytes changed.
    fn sync_cell_preview(&mut self, allow_existing: bool, cx: &mut Context<Self>) -> bool {
        let Some(projected) = self.projected_cell(allow_existing, cx) else {
            return self.revert_cell_preview();
        };
        let same = self
            .open
            .as_ref()
            .is_some_and(|open| open.document.serialize() == projected.serialize());
        if same {
            return false;
        }
        let kind = self.editor.as_ref().map(|editor| editor.kind_tag());
        let key_text = self
            .editor
            .as_ref()
            .and_then(|editor| editor.key.as_ref())
            .map(|input| input.read(cx).value().trim().to_string())
            .unwrap_or_default();
        let preview_node = if matches!(kind, Some(EditTag::New)) {
            projected
                .entries()
                .filter(|(_, entry)| entry.key == key_text)
                .map(|(index, _)| index)
                .last()
        } else {
            self.editor.as_ref().and_then(|editor| editor.node)
        };
        let armed = self.editor.as_ref().is_some_and(|editor| editor.undo_armed);
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        if !armed {
            open.arm_undo();
        }
        open.document = projected;
        if let Some(editor) = self.editor.as_mut() {
            editor.undo_armed = true;
            editor.preview_node = preview_node;
        }
        true
    }

    fn revert_cell_preview(&mut self) -> bool {
        let Some(editor) = self.editor.as_ref() else {
            return false;
        };
        if !editor.undo_armed {
            return false;
        }
        let baseline = editor.baseline.clone();
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        let changed = open.document.serialize() != baseline.serialize();
        open.restore_baseline(baseline, true);
        if let Some(editor) = self.editor.as_mut() {
            editor.undo_armed = false;
            editor.preview_node = None;
        }
        changed
    }

    fn projected_cell(&self, allow_existing: bool, cx: &App) -> Option<EnvDocument> {
        let editor = self.editor.as_ref()?;
        let mut doc = editor.baseline.clone();
        let value = editor.value.read(cx).value().to_string();
        match editor.kind_tag() {
            EditTag::Value => {
                let node = editor.node?;
                let unchanged = matches!(
                    doc.nodes.get(node),
                    Some(Node::Entry(entry)) if entry.value == value
                );
                if unchanged {
                    return None;
                }
                if let Some(Node::Entry(entry)) = doc.nodes.get_mut(node) {
                    entry.set_value(value);
                }
                Some(doc)
            }
            EditTag::Rename => {
                let key = editor
                    .key
                    .as_ref()
                    .map(|input| input.read(cx).value().trim().to_string())
                    .unwrap_or_default();
                let node = editor.node?;
                if !is_env_key(&key) {
                    return None;
                }
                if doc
                    .entries()
                    .any(|(index, entry)| index != node && entry.key == key)
                {
                    return None;
                }
                let unchanged =
                    matches!(doc.nodes.get(node), Some(Node::Entry(entry)) if entry.key == key);
                if unchanged {
                    return None;
                }
                if let Some(Node::Entry(entry)) = doc.nodes.get_mut(node) {
                    entry.set_key(&key);
                }
                Some(doc)
            }
            EditTag::New => {
                let key = editor
                    .key
                    .as_ref()
                    .map(|input| input.read(cx).value().trim().to_string())
                    .unwrap_or_default();
                if !is_env_key(&key) {
                    return None;
                }
                if doc.contains_key(&key) {
                    if !allow_existing {
                        return None;
                    }
                    doc.set_value(&key, &value);
                } else {
                    doc.push_entry(key, value);
                }
                Some(doc)
            }
        }
    }

    fn copy_selected(&mut self, cx: &mut Context<Self>) {
        if self.list_keys_blocked() {
            return;
        }
        let Some(node) = self.selected_node() else {
            return;
        };
        let value = match self
            .open
            .as_ref()
            .and_then(|open| open.document.nodes.get(node))
        {
            Some(Node::Entry(entry)) => entry.value.clone(),
            _ => return,
        };
        cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
        self.arm_clear(value, cx);
        self.acknowledge_copy(&row_copy_id(node), cx);
        self.status = if self.config.settings.clear_clipboard {
            "Copied. Clipboard clears in 30s.".into()
        } else {
            "Copied.".into()
        };
        cx.notify();
    }

    fn copy_all(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self
            .open
            .as_ref()
            .map(|open| open.document.export_enabled())
        else {
            return;
        };
        if text.is_empty() {
            self.status = "No keys to copy.".into();
            cx.notify();
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        self.arm_clear(text, cx);
        self.acknowledge_copy(TOOLBAR_COPY_ID, cx);
        self.status = "Copied.".into();
        cx.notify();
    }

    fn acknowledge_copy(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(file) = self.open.as_ref().map(|open| open.path.clone()) else {
            return;
        };
        let generation = self.copy_acks.acknowledge(id, file);
        let key = id.to_string();
        cx.spawn(async move |this, async_cx| {
            async_cx
                .background_executor()
                .timer(crate::copy_feedback::copy_ack_lifetime())
                .await;
            let _ = this.update(async_cx, |view, cx| {
                if view.copy_acks.expire(&key, generation) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn copy_ack(&self, id: &str) -> Option<&CopyAck> {
        let file = self.open.as_ref()?;
        self.copy_acks.visible(id, &file.path)
    }

    fn arm_clear(&mut self, value: String, cx: &mut Context<Self>) {
        if !self.config.settings.clear_clipboard {
            self.clear_task = None;
            return;
        }
        self.clear_token = self.clear_token.wrapping_add(1);
        let token = self.clear_token;
        self.clear_task = Some(cx.spawn(async move |this, async_cx| {
            async_cx.background_executor().timer(CLEAR_AFTER).await;
            let _ = async_cx.update(|cx| {
                let _ = this.update(cx, |view, cx| {
                    if view.clear_token != token {
                        return;
                    }
                    let still_ours = cx
                        .read_from_clipboard()
                        .and_then(|item| item.text())
                        .is_some_and(|text| text == value);
                    if still_ours {
                        cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
                        view.status = "Clipboard cleared.".into();
                        cx.notify();
                    }
                });
            });
        }));
    }

    fn toggle_reveal(&mut self, cx: &mut Context<Self>) {
        let key = if self.diff_open {
            self.diff_report
                .as_ref()
                .and_then(|report| report.rows.get(self.diff_cursor))
                .map(|row| row.key.clone())
        } else {
            self.selected_key()
        };
        let Some(key) = key else {
            return;
        };
        if !self.revealed.remove(&key) {
            self.revealed.insert(key);
        }
        cx.notify();
    }

    fn toggle_show_all(&mut self, cx: &mut Context<Self>) {
        self.show_all = !self.show_all;
        self.status = if self.show_all {
            "Showing all values.".into()
        } else {
            "Values hidden.".into()
        };
        cx.notify();
    }

    fn toggle_disabled_selected(&mut self, cx: &mut Context<Self>) {
        if self.diff_open {
            return;
        }
        let Some(node) = self.selected_node() else {
            return;
        };
        self.toggle_disabled_node(node, cx);
    }

    fn toggle_disabled_node(&mut self, node: usize, cx: &mut Context<Self>) {
        self.commit_cell_if_open(cx);
        let key = match self
            .open
            .as_ref()
            .and_then(|open| open.document.nodes.get(node))
        {
            Some(Node::Entry(entry)) => entry.key.clone(),
            _ => return,
        };
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let mut now_disabled = None;
        open.mutate(|document| {
            now_disabled = document.toggle_disabled_at(node);
        });
        let Some(disabled) = now_disabled else {
            return;
        };
        // Keep selection on the toggled row even if pairing moved its visual position.
        if let Some(pos) = self
            .visible_entry_nodes()
            .iter()
            .position(|&candidate| candidate == node)
        {
            self.cursor = pos;
        }
        self.mark_edited(cx);
        self.status = if disabled {
            format!("Turned off {key}.")
        } else {
            format!("Turned on {key}.")
        };
        cx.notify();
    }

    fn ask_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.list_keys_blocked() {
            return;
        }
        self.commit_cell_if_open(cx);
        let Some(node) = self.selected_node() else {
            return;
        };
        let Some(key) = self.selected_key() else {
            return;
        };
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.mutate(|document| {
            document.remove_entry_at(node);
        });
        let len = self.visible_entry_nodes().len();
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor >= len {
            self.cursor = len - 1;
        }
        self.mark_edited(cx);
        self.status = format!("Removed {key}. Ctrl+Z brings it back.");
        self.focus_self(window, cx);
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        self.commit_cell_if_open(cx);
        self.finalize_text_session();
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if !open.undo() {
            self.status = "Nothing to undo.".into();
            cx.notify();
            return;
        }
        self.text_editor_needs_sync = true;
        self.sync_keys();
        self.io_epoch = self.io_epoch.wrapping_add(1);
        self.diff_stale = true;
        self.enqueue_save(cx);
        self.status = "Undone.".into();
        cx.notify();
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        self.commit_cell_if_open(cx);
        self.finalize_text_session();
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if !open.redo() {
            return;
        }
        self.text_editor_needs_sync = true;
        self.sync_keys();
        self.io_epoch = self.io_epoch.wrapping_add(1);
        self.diff_stale = true;
        self.enqueue_save(cx);
        self.status = "Redone.".into();
        cx.notify();
    }

    fn reload_external(&mut self, cx: &mut Context<Self>) {
        self.begin_reload(false, cx);
    }

    fn begin_reload(&mut self, keep_cursor: bool, cx: &mut Context<Self>) {
        let Some(open) = &self.open else {
            return;
        };
        if !open.external_change {
            return;
        }
        self.reload_keep_cursor = keep_cursor;
        self.io_epoch = self.io_epoch.wrapping_add(1);
        let epoch = self.io_epoch;
        let path = open.path.clone();
        self.status = "Reloading…".into();
        self.io_queue.push_back(IoJob::Reload { epoch, path });
        self.pump_io(cx);
    }

    fn open_add_folder(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add folder".into()),
        });
        self.status = "Choose a folder.".into();
        cx.notify();
        cx.spawn(async move |this, async_cx| {
            let picked = receiver.await;
            let _ = this.update(async_cx, |view, cx| match picked {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        view.commit_folder(path.display().to_string(), cx);
                    }
                }
                Ok(Ok(None)) => {
                    view.status = "Add folder cancelled.".into();
                    cx.notify();
                }
                Ok(Err(err)) => {
                    view.status = format!("Couldn't open the folder picker ({err}). Type a path.");
                    view.show_path_panel(cx);
                }
                Err(_) => {}
            });
        })
        .detach();
    }

    fn show_path_panel(&mut self, cx: &mut Context<Self>) {
        self.overlay = Overlay::AddFolder;
        self.window
            .update(cx, |_, window, cx| {
                self.folder_input.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                });
                let handle = self.folder_input.focus_handle(cx);
                window.focus(&handle, cx);
            })
            .ok();
        cx.notify();
    }

    fn commit_folder(&mut self, path: String, cx: &mut Context<Self>) {
        let path = PathBuf::from(path.trim());
        if path.as_os_str().is_empty() {
            self.status = "That folder does not exist.".into();
            cx.notify();
            return;
        }
        self.overlay = Overlay::None;
        self.scanning = true;
        self.status = "Adding folder…".into();
        self.focus_self_cx(cx);
        let mut config = self.config.clone();
        cx.notify();
        cx.spawn(async move |this, async_cx| {
            let prepared = async_cx
                .background_executor()
                .spawn(async move {
                    if !path.is_dir() {
                        return Err("That folder does not exist.".to_string());
                    }
                    config.add_folder(path);
                    config
                        .save()
                        .map_err(|err| format!("Could not save settings: {err}"))?;
                    Ok(config)
                })
                .await;
            let _ = this.update(async_cx, |view, cx| match prepared {
                Ok(config) => {
                    view.config = config;
                    view.status = "Folder added.".into();
                    view.begin_scan(false, cx);
                }
                Err(err) => {
                    view.scanning = false;
                    view.status = err;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn remove_folder(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.config.folders.len() {
            return;
        }
        let folder = self.config.folders[index].clone();
        self.config.remove_folder(&folder);
        self.persist_config(cx);
        self.status = "Folder removed.".into();
        self.begin_scan(false, cx);
        cx.notify();
    }

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::Settings;
        let spec = self.config.settings.global_hotkey.clone();
        self.hotkey_input.update(cx, |input, cx| {
            input.set_value(spec, window, cx);
        });
        cx.notify();
    }

    fn set_theme(&mut self, theme: ThemeSetting, window: &mut Window, cx: &mut Context<Self>) {
        self.config.settings.theme = theme;
        self.persist_config(cx);
        self.apply_theme(window, cx);
        cx.notify();
    }

    fn toggle_clear_clipboard(&mut self, cx: &mut Context<Self>) {
        self.config.settings.clear_clipboard = !self.config.settings.clear_clipboard;
        self.persist_config(cx);
        self.status = if self.config.settings.clear_clipboard {
            "Clipboard clears 30s after a copy.".into()
        } else {
            "Clipboard is left alone after a copy.".into()
        };
        cx.notify();
    }

    fn commit_hotkey(&mut self, spec: String, window: &mut Window, cx: &mut Context<Self>) {
        let spec = spec.trim().to_string();
        self.focus_self(window, cx);
        self.install_hotkey(spec, true, cx);
    }

    fn toggle_diff(&mut self, cx: &mut Context<Self>) {
        if self.list_keys_blocked() {
            return;
        }
        if self.diff_open {
            self.clear_compare();
            cx.notify();
            return;
        }
        self.diff_left = None;
        self.diff_right = None;
        self.choose_diff_sides();
        if self.diff_left.is_none() || self.diff_left == self.diff_right {
            self.status = "This project has one env file.".into();
            cx.notify();
            return;
        }
        self.diff_open = true;
        self.load_diff(cx);
        cx.notify();
    }

    fn cycle_diff(&mut self, cx: &mut Context<Self>) {
        if self.file_count() < 2 {
            self.status = "This project has one env file.".into();
            cx.notify();
            return;
        }
        if !self.diff_open {
            self.toggle_diff(cx);
            return;
        }
        let file_count = self.file_count();
        let left = self.diff_left.unwrap_or(0);
        let right = self.diff_right.unwrap_or(0);
        let mut next = (right + 1) % file_count;
        if next == left {
            next = (next + 1) % file_count;
        }
        self.diff_right = Some(next);
        self.load_diff(cx);
        cx.notify();
    }

    fn file_prompt_input(&self) -> Option<&Entity<TextInput>> {
        match &self.file_prompt {
            FilePrompt::Create { input, .. } | FilePrompt::Rename { input, .. } => Some(input),
            FilePrompt::None => None,
        }
    }

    fn file_prompt_focused(&self, window: &Window, cx: &App) -> bool {
        self.file_prompt_input()
            .is_some_and(|input| input.focus_handle(cx).is_focused(window))
    }

    fn close_file_prompt(&mut self) {
        self.file_prompt = FilePrompt::None;
        self.file_prompt_sub = None;
    }

    fn set_file_prompt_error(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        let message = message.into();
        match &mut self.file_prompt {
            FilePrompt::Create { error, .. } | FilePrompt::Rename { error, .. } => {
                *error = Some(message.clone());
            }
            FilePrompt::None => {}
        }
        self.status = message;
        cx.notify();
    }

    fn clear_file_prompt_error(&mut self) {
        match &mut self.file_prompt {
            FilePrompt::Create { error, .. } | FilePrompt::Rename { error, .. } => *error = None,
            FilePrompt::None => {}
        }
    }

    fn focus_file_prompt(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.file_prompt_input().cloned() else {
            return;
        };
        input.update(cx, |state, cx| state.select_all(window, cx));
        let handle = input.focus_handle(cx);
        window.focus(&handle, cx);
    }

    fn open_file_prompt(
        &mut self,
        prompt: FilePrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = (match &prompt {
            FilePrompt::Create { input, .. } | FilePrompt::Rename { input, .. } => {
                Some(input.clone())
            }
            FilePrompt::None => None,
        }) else {
            return;
        };
        self.close_file_prompt();
        self.file_prompt_sub =
            Some(
                cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                    TextInputEvent::Submit => this.commit_file_prompt(window, cx),
                    TextInputEvent::Change => {
                        this.clear_file_prompt_error();
                        cx.notify();
                    }
                    TextInputEvent::ExitDown => {
                        if let FilePrompt::Create { source_focus, .. } = &mut this.file_prompt {
                            *source_focus = true;
                        }
                        this.focus_self(window, cx);
                        cx.notify();
                    }
                    TextInputEvent::Focus => {
                        if let FilePrompt::Create { source_focus, .. } = &mut this.file_prompt {
                            *source_focus = false;
                        }
                    }
                    TextInputEvent::Blur => {}
                }),
            );
        self.file_prompt = prompt;
        self.focus_file_prompt(window, cx);
        cx.notify();
    }

    fn begin_create_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .project_ix
            .and_then(|ix| self.projects.get(ix))
            .is_none()
        {
            self.status = "Open a project first.".into();
            cx.notify();
            return;
        }
        self.open_new_file_prompt(window, cx);
    }

    fn open_new_file_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.is_some() {
            self.commit_editor(None, false, window, cx);
        }
        self.tab_menu = None;
        let name = next_env_filename(&self.current_file_names());
        let input = cx.new(|cx| TextInput::new("File name", cx).with_text(name).exit_down());
        self.open_file_prompt(
            FilePrompt::Create {
                input,
                error: None,
                source: 0,
                keys_only: true,
                source_focus: false,
            },
            window,
            cx,
        );
    }

    fn begin_rename_file(&mut self, file_ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self
            .project_ix
            .and_then(|ix| self.projects.get(ix))
            .and_then(|project| project.files.get(file_ix))
            .map(|file| file.name.clone())
        else {
            return;
        };
        if self.editor.is_some() {
            self.commit_editor(None, false, window, cx);
        }
        let input = cx.new(|cx| TextInput::new("File name", cx).with_text(name));
        self.open_file_prompt(
            FilePrompt::Rename {
                file_ix,
                input,
                error: None,
            },
            window,
            cx,
        );
    }

    fn current_file_names(&self) -> Vec<String> {
        self.project_ix
            .and_then(|ix| self.projects.get(ix))
            .map(|project| project.files.iter().map(|file| file.name.clone()).collect())
            .unwrap_or_default()
    }

    fn list_keys_blocked(&self) -> bool {
        self.tab_menu.is_some()
            || !matches!(self.overlay, Overlay::None)
            || self.create_source_focused()
    }

    fn create_source_focused(&self) -> bool {
        matches!(
            self.file_prompt,
            FilePrompt::Create {
                source_focus: true,
                ..
            }
        )
    }

    fn step_create_source(
        &mut self,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.create_source_focused() {
            return false;
        }
        let count = self.current_file_names().len();
        let source = match &self.file_prompt {
            FilePrompt::Create { source, .. } => *source,
            _ => return false,
        };
        if source == 0 && delta < 0 {
            if let FilePrompt::Create { source_focus, .. } = &mut self.file_prompt {
                *source_focus = false;
            }
            self.focus_file_prompt(window, cx);
            cx.notify();
            return true;
        }
        let next = step_index(source, count + 1, delta);
        if let FilePrompt::Create { source, .. } = &mut self.file_prompt {
            *source = next;
        }
        cx.notify();
        true
    }

    fn nudge_copy_mode(&mut self, keys_only: bool, cx: &mut Context<Self>) -> bool {
        if !self.create_source_focused() {
            return false;
        }
        if let Some(name) = self.selected_copy_name() {
            if !is_example_filename(&name) {
                if let FilePrompt::Create {
                    keys_only: mode, ..
                } = &mut self.file_prompt
                {
                    *mode = keys_only;
                }
            }
        }
        cx.notify();
        true
    }

    fn selected_copy_name(&self) -> Option<String> {
        let FilePrompt::Create { source, .. } = &self.file_prompt else {
            return None;
        };
        if *source == 0 {
            return None;
        }
        let project = self.project_ix.and_then(|ix| self.projects.get(ix))?;
        let names: Vec<&str> = project
            .files
            .iter()
            .map(|file| file.name.as_str())
            .collect();
        let index = copy_source_order(&names).get(source - 1).copied()?;
        project.files.get(index).map(|file| file.name.clone())
    }

    fn open_tab_menu(&mut self, file_ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let exists = self
            .project_ix
            .and_then(|ix| self.projects.get(ix))
            .is_some_and(|project| file_ix < project.files.len());
        if !exists {
            return;
        }
        self.close_file_prompt();
        self.commit_cell_if_open(cx);
        self.tab_menu = Some(TabMenu { file_ix, cursor: 0 });
        self.zone = Zone::Tabs;
        self.tab_cursor = file_ix;
        self.focus_self(window, cx);
        cx.notify();
    }

    fn close_tab_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab_menu.is_none() {
            return;
        }
        self.tab_menu = None;
        self.focus_self(window, cx);
        cx.notify();
    }

    fn activate_tab_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(menu) = self.tab_menu.take() else {
            return false;
        };
        match menu.cursor {
            0 => self.begin_rename_file(menu.file_ix, window, cx),
            1 => self.duplicate_tab(menu.file_ix, cx),
            2 => self.reveal_tab(menu.file_ix, cx),
            _ => self.ask_delete_tab(menu.file_ix, window, cx),
        }
        cx.notify();
        true
    }

    fn ask_delete_tab(&mut self, file_ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.current_tab_labels().get(file_ix).map(|label| {
            if label.suffix.is_empty() {
                label.name.clone()
            } else {
                label.relative.clone()
            }
        }) else {
            return;
        };
        self.overlay = Overlay::DeleteFile { file_ix, name };
        self.focus_self(window, cx);
        cx.notify();
    }

    fn delete_tab(&mut self, file_ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        let Some(path) = self
            .project_ix
            .and_then(|ix| self.projects.get(ix))
            .and_then(|project| project.files.get(file_ix))
            .map(|file| file.path.clone())
        else {
            self.focus_self(window, cx);
            return;
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.io_epoch = self.io_epoch.wrapping_add(1);
        self.io_queue.retain(|job| match job {
            IoJob::Save { path: saved, .. }
            | IoJob::Load { path: saved, .. }
            | IoJob::Reload { path: saved, .. } => saved != &path,
            _ => true,
        });
        if self.open.as_ref().is_some_and(|open| open.path == path) {
            self.open = None;
            self.close_editor();
            self.text_session_before = None;
            self.text_editor_needs_sync = true;
        }
        self.detach_path(&path);
        if self.file_ix.is_some() {
            self.load_selected_file(cx);
        }
        self.io_queue.push_back(IoJob::Delete { path, name });
        self.pump_io(cx);
        self.focus_self(window, cx);
        cx.notify();
    }

    fn detach_path(&mut self, path: &Path) {
        let Some(project) = self.project_ix.and_then(|ix| self.projects.get_mut(ix)) else {
            return;
        };
        let Some(index) = project.files.iter().position(|file| file.path == path) else {
            return;
        };
        project.files.remove(index);
        let len = project.files.len();
        self.file_ix = match self.file_ix {
            Some(ix) if ix == index => {
                if len == 0 {
                    None
                } else {
                    Some(index.min(len - 1))
                }
            }
            Some(ix) if ix > index => Some(ix - 1),
            other => other,
        };
        self.tab_cursor = self.file_ix.unwrap_or(0);
        Self::shift_side(&mut self.diff_left, index);
        Self::shift_side(&mut self.diff_right, index);
        if self.diff_open && (self.diff_left.is_none() || self.diff_right.is_none()) {
            self.diff_open = false;
            self.diff_left = None;
            self.diff_right = None;
            self.diff_report = None;
        }
    }

    fn shift_side(side: &mut Option<usize>, removed: usize) {
        match *side {
            Some(ix) if ix == removed => *side = None,
            Some(ix) if ix > removed => *side = Some(ix - 1),
            _ => {}
        }
    }

    fn duplicate_tab(&mut self, file_ix: usize, cx: &mut Context<Self>) {
        self.commit_cell_if_open(cx);
        let Some(path) = self
            .project_ix
            .and_then(|ix| self.projects.get(ix))
            .and_then(|project| project.files.get(file_ix))
            .map(|file| file.path.clone())
        else {
            return;
        };
        let Some(parent) = path.parent().map(|parent| parent.to_path_buf()) else {
            return;
        };
        let name = next_env_filename(&self.current_file_names());
        let dest = parent.join(&name);
        let text = self
            .open
            .as_ref()
            .filter(|open| open.path == path)
            .map(|open| open.document.serialize());
        cx.spawn(async move |this, async_cx| {
            let copied = async_cx
                .background_executor()
                .spawn(async move {
                    if let Some(text) = text {
                        OpenFile::create(&dest, EnvDocument::parse(&text)).map(|_| dest)
                    } else {
                        std::fs::copy(&path, &dest).map(|_| dest)
                    }
                })
                .await;
            let _ = this.update(async_cx, |view, cx| {
                match copied {
                    Ok(path) => {
                        view.pending_file = Some(path);
                        view.status = format!("Duplicated as {name}.");
                        view.begin_scan(true, cx);
                    }
                    Err(_) => view.status = "Couldn't duplicate that file.".into(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn reveal_tab(&mut self, file_ix: usize, cx: &mut Context<Self>) {
        let Some(path) = self
            .project_ix
            .and_then(|ix| self.projects.get(ix))
            .and_then(|project| project.files.get(file_ix))
            .map(|file| file.path.clone())
        else {
            return;
        };
        cx.spawn(async move |this, async_cx| {
            let opened = async_cx
                .background_executor()
                .spawn(async move { open_containing_folder(&path) })
                .await;
            let _ = this.update(async_cx, |view, cx| {
                if opened.is_err() {
                    view.status = "Couldn't show that file.".into();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn commit_file_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let names = self.current_file_names();
        let (creating, file_ix, text) = match &self.file_prompt {
            FilePrompt::Create { input, .. } => (true, None, input.read(cx).value().to_string()),
            FilePrompt::Rename { file_ix, input, .. } => {
                (false, Some(*file_ix), input.read(cx).value().to_string())
            }
            FilePrompt::None => return,
        };
        let ignoring = file_ix.and_then(|index| names.get(index)).cloned();
        let name = match check_env_filename(&text, &names, ignoring.as_deref()) {
            Ok(name) => name,
            Err(EnvFileNameError::Empty) => {
                self.set_file_prompt_error("Enter a file name.", cx);
                return;
            }
            Err(EnvFileNameError::NotEnv | EnvFileNameError::Nested) => {
                self.set_file_prompt_error("Use a name like .env or .env.local.", cx);
                return;
            }
            Err(EnvFileNameError::Collision) => {
                self.set_file_prompt_error("A file with that name already exists.", cx);
                return;
            }
        };
        if ignoring.as_deref() == Some(name.as_str()) {
            self.close_file_prompt();
            self.focus_self(window, cx);
            cx.notify();
            return;
        }
        let Some(project_ix) = self.project_ix else {
            return;
        };
        let Some(root) = self
            .projects
            .get(project_ix)
            .map(|project| project.root.clone())
        else {
            return;
        };
        if creating {
            let (body, dest_dir) = self.new_file_body(project_ix, root);
            let dest = dest_dir.join(&name);
            self.close_file_prompt();
            self.focus_self(window, cx);
            self.spawn_create_file(dest, name, body, cx);
        } else {
            let Some(project) = self.projects.get(project_ix) else {
                return;
            };
            let Some(file_ix) = file_ix else {
                return;
            };
            let Some(from) = project.files.get(file_ix).map(|file| file.path.clone()) else {
                return;
            };
            let Some(parent) = from.parent().map(|parent| parent.to_path_buf()) else {
                return;
            };
            let dest = parent.join(&name);
            self.close_file_prompt();
            self.focus_self(window, cx);
            self.spawn_rename_file(from, dest, name, cx);
        }
    }

    fn new_file_body(&self, project_ix: usize, root: PathBuf) -> (NewFileBody, PathBuf) {
        let FilePrompt::Create {
            source, keys_only, ..
        } = &self.file_prompt
        else {
            return (NewFileBody::Empty, root);
        };
        if *source == 0 {
            return (NewFileBody::Empty, root);
        }
        let Some(project) = self.projects.get(project_ix) else {
            return (NewFileBody::Empty, root);
        };
        let names: Vec<&str> = project
            .files
            .iter()
            .map(|file| file.name.as_str())
            .collect();
        let Some(file_index) = copy_source_order(&names).get(source - 1).copied() else {
            return (NewFileBody::Empty, root);
        };
        let Some(file) = project.files.get(file_index) else {
            return (NewFileBody::Empty, root);
        };
        let parent = file
            .path
            .parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or(root);
        (
            NewFileBody::Copy {
                source: file.path.clone(),
                values: copy_values_for(&file.name, *keys_only),
            },
            parent,
        )
    }

    fn spawn_create_file(
        &mut self,
        dest: PathBuf,
        name: String,
        body: NewFileBody,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, async_cx| {
            let created = async_cx
                .background_executor()
                .spawn(async move {
                    let document = match &body {
                        NewFileBody::Empty => EnvDocument::parse(""),
                        NewFileBody::Copy { source, values } => {
                            let loaded = OpenFile::load(source)?;
                            copied_document(&loaded.document, *values)
                        }
                    };
                    OpenFile::create(&dest, document).map(|_| dest)
                })
                .await;
            let _ = this.update(async_cx, |view, cx| {
                match created {
                    Ok(path) => {
                        view.pending_file = Some(path);
                        view.status = format!("Created {name}.");
                        view.begin_scan(true, cx);
                    }
                    Err(_) => view.status = "Couldn't create that file.".into(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn spawn_rename_file(
        &mut self,
        from: PathBuf,
        dest: PathBuf,
        name: String,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, async_cx| {
            let renamed = async_cx
                .background_executor()
                .spawn(async move {
                    rename_env_file(&from, &dest)?;
                    let mtime = std::fs::metadata(&dest)
                        .and_then(|meta| meta.modified())
                        .ok();
                    Ok::<_, std::io::Error>((from, dest, mtime))
                })
                .await;
            let _ = this.update(async_cx, |view, cx| {
                match renamed {
                    Ok((from, dest, mtime)) => {
                        view.retarget_file(&from, &dest, mtime);
                        view.status = format!("Renamed to {name}.");
                        view.begin_scan(true, cx);
                    }
                    Err(_) => view.status = "Couldn't rename that file.".into(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn retarget_file(&mut self, from: &Path, to: &Path, mtime: Option<SystemTime>) {
        let name = to
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        for project in &mut self.projects {
            if let Some(file) = project.files.iter_mut().find(|file| file.path == from) {
                file.path = to.to_path_buf();
                file.name = name.clone();
                file.prod = is_prod_filename(&file.name);
            }
        }
        if let Some(open) = self.open.as_mut() {
            if open.path == from {
                open.path = to.to_path_buf();
                if mtime.is_some() {
                    open.mtime = mtime;
                }
                open.external_change = false;
            }
        }
    }

    fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.status = "Clipboard is empty.".into();
            cx.notify();
            return;
        };
        if !self.stage_paste(text) {
            self.status = "That clipboard is not a block of KEY=value lines.".into();
        }
        cx.notify();
    }

    fn stage_paste(&mut self, text: String) -> bool {
        let Some(open) = &self.open else {
            self.status = "Open a file first.".into();
            return true;
        };
        let Some(plan) = plan_paste(&open.document, &text) else {
            return false;
        };
        if plan.is_empty() {
            return false;
        }
        self.overlay = Overlay::Paste(plan);
        self.status = "Enter adds these keys. Esc cancels.".into();
        true
    }

    fn apply_overlay_paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_cell_if_open(cx);
        let Overlay::Paste(plan) = &self.overlay else {
            return;
        };
        let plan = plan.clone();
        self.overlay = Overlay::None;
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.apply_paste(&plan);
        self.mark_edited(cx);
        self.status = format!(
            "Pasted {} added, {} overwritten.",
            plan.added(),
            plan.overwritten()
        );
        self.focus_self(window, cx);
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_project_on_open = false;
        let mods = &event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        if mods.control && !mods.alt && !mods.platform && matches!(self.overlay, Overlay::None) {
            if key == "tab" {
                self.move_file(if mods.shift { -1 } else { 1 }, window, cx);
                window.prevent_default();
                cx.stop_propagation();
                return;
            }
            if !mods.shift {
                if let Ok(n) = key.parse::<usize>() {
                    if (1..=9).contains(&n) {
                        self.activate_tab(n - 1, true, window, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                }
            }
        }
        if mods.control || mods.alt || mods.platform {
            return;
        }
        if self.editor.is_some()
            && matches!(self.overlay, Overlay::None)
            && (key == "tab" || is_cell_commit_key(key))
        {
            let motion = if key == "tab" && mods.shift {
                Some(CellMotion::PrevCell)
            } else if key == "tab" {
                Some(CellMotion::NextCell)
            } else if is_cell_commit_key(key) && !mods.shift {
                // Nothing in the list is chosen until a click. Enter commits.
                match enter_while_suggesting(None) {
                    EnterEffect::CommitAndClose => Some(CellMotion::NextRow),
                    EnterEffect::AcceptSuggestion(index) => {
                        self.accept_suggestion(index, window, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                }
            } else {
                None
            };
            if let Some(motion) = motion {
                self.apply_cell_motion(motion, window, cx);
                window.prevent_default();
                cx.stop_propagation();
                return;
            }
        }
        match key {
            "tab" if self.file_prompt_focused(window, cx) => {
                self.commit_file_prompt(window, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "tab"
                if matches!(self.overlay, Overlay::None)
                    && (self.filter_focused(Zone::Projects, window, cx)
                        || self.filter_focused(Zone::Keys, window, cx)) =>
            {
                self.switch_zone(!mods.shift, window, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "escape" => {
                self.cancel(window, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            _ if is_cell_commit_key(key)
                && mods.shift
                && self.editor.is_none()
                && self.zone == Zone::Tabs =>
            {
                self.toggle_compare_tab(self.tab_cursor, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            _ if is_cell_commit_key(key) && self.capture_enter() => {
                self.confirm(window, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn capture_enter(&self) -> bool {
        matches!(self.overlay, Overlay::Paste(_))
    }

    fn missing_names(&self) -> Vec<String> {
        let Some(project) = self.project_ix.and_then(|ix| self.projects.get(ix)) else {
            return Vec::new();
        };
        let Some(file) = self.file_ix.and_then(|ix| project.files.get(ix)) else {
            return Vec::new();
        };
        if is_example_filename(&file.name) {
            return Vec::new();
        }
        let Some(example) = project
            .files
            .iter()
            .find(|item| is_example_filename(&item.name))
        else {
            return Vec::new();
        };
        example
            .active_keys
            .iter()
            .filter(|key| !file.keys.iter().any(|have| have == *key))
            .cloned()
            .collect()
    }

    fn visible(&self, key: &str) -> bool {
        self.show_all || self.revealed.contains(key)
    }
}

#[derive(Clone, Copy)]
enum EditTag {
    Value,
    Rename,
    New,
}

impl Editor {
    fn kind_tag(&self) -> EditTag {
        match self.kind {
            EditKind::Value => EditTag::Value,
            EditKind::Rename => EditTag::Rename,
            EditKind::New => EditTag::New,
        }
    }
}

fn recompute_missing(project: &mut Project) {
    let example = project
        .files
        .iter()
        .find(|file| is_example_filename(&file.name))
        .map(|file| file.active_keys.clone());
    let Some(example) = example else {
        for file in &mut project.files {
            file.missing_vs_example = false;
        }
        project.missing_vs_example = false;
        return;
    };
    for file in &mut project.files {
        file.missing_vs_example = !is_example_filename(&file.name)
            && example
                .iter()
                .any(|key| !file.keys.iter().any(|have| have == key));
    }
    project.missing_vs_example = project.files.iter().any(|file| file.missing_vs_example);
}

fn masked(value: &str) -> String {
    let count = value.chars().count();
    if count == 0 {
        return String::new();
    }
    "•".repeat(count.min(MASK_MAX_DOTS))
}

fn display_branch(branch: &str) -> Option<&str> {
    let trimmed = branch.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("unknown")
        || matches!(trimmed, "—" | "-" | "…")
    {
        None
    } else {
        Some(trimmed)
    }
}

fn write_entries(document: &mut EnvDocument, entries: &[(String, String, bool)]) {
    for (key, value, disabled) in entries {
        document.upsert_entry(key.clone(), value.clone(), *disabled);
    }
}

fn fuzzy_hit(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let haystack = haystack.to_ascii_lowercase();
    if haystack.contains(needle) {
        return true;
    }
    let mut rest = haystack.chars();
    needle
        .chars()
        .all(|ch| rest.any(|candidate| candidate == ch))
}

fn display_value(value: &str, visible: bool) -> String {
    if value.is_empty() {
        return "empty".into();
    }
    if visible {
        value.replace('\n', "\\n")
    } else {
        masked(value)
    }
}

impl Focusable for EnvyApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EnvyApp {
    fn tint_fields(&self, accent: gpui_kit::Hsla, cx: &mut Context<Self>) {
        let mut paint = |input: &Entity<TextInput>| {
            input.update(cx, |field, _| field.set_accent(accent));
        };
        paint(&self.folder_input);
        paint(&self.hotkey_input);
        paint(&self.project_filter);
        paint(&self.key_filter);
        if let Some(editor) = &self.editor {
            if let Some(key) = &editor.key {
                paint(key);
            }
            paint(&editor.value);
        }
        if let Some(input) = self.file_prompt_input() {
            paint(input);
        }
    }
}

impl Render for EnvyApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_text_editor_if_needed(window, cx);
        let p = Palette::for_mode(cx.theme().mode.is_dark());
        self.tint_fields(p.accent, cx);
        let mono = cx.theme().mono_font_family.clone();
        let ui_font = cx.theme().font_family.clone();

        let sidebar = div()
            .relative()
            .w(px(self.sidebar_width))
            .h_full()
            .flex_shrink_0()
            .child(self.render_sidebar(p, window, cx))
            .child(Self::resize_handle(
                "sidebar-resize",
                ResizeKind::Sidebar,
                p,
                cx.entity(),
            ));
        let center = if self.booting && self.projects.is_empty() {
            v_flex()
                .flex_1()
                .h_full()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .bg(p.bg_canvas)
                .text_color(p.fg_tertiary)
                .child(
                    Icon::new(IconName::LoaderCircle)
                        .size(px(18.))
                        .text_color(p.accent),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .child("Starting"),
                )
        } else if self.projects.is_empty() && !self.scanning {
            self.render_empty_state(p, mono.clone(), cx)
        } else if self.open.is_none() && !self.diff_open {
            let tabs = self.render_tabs(p, mono.clone(), cx);
            let footer = self.render_footer(p, mono.clone(), cx);
            v_flex()
                .flex_1()
                .h_full()
                .min_w(px(0.))
                .bg(p.bg_canvas)
                .text_color(p.fg_primary)
                .child(tabs)
                .child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .text_color(p.fg_tertiary)
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .child("Select a file"),
                        ),
                )
                .child(footer)
        } else {
            let missing = self.missing_names();
            let tabs = self.render_tabs(p, mono.clone(), cx);
            let toolbar = self.render_toolbar(p, missing.len(), window, cx);
            let body = if self.diff_open {
                self.render_diff_view(p, mono.clone(), cx)
            } else if self.editor_view == EditorView::Text {
                self.render_text_editor(p, mono.clone(), cx)
            } else {
                self.render_key_table(p, mono.clone(), window, cx)
            };
            let footer = self.render_footer(p, mono.clone(), cx);
            v_flex()
                .flex_1()
                .h_full()
                .min_w(px(0.))
                .bg(p.bg_canvas)
                .text_color(p.fg_primary)
                .child(tabs)
                .child(toolbar)
                .child(body)
                .child(footer)
        };

        let mut root = h_flex()
            .id("envy-root")
            .key_context("Envy")
            .track_focus(&self.focus)
            .flex_1()
            .w_full()
            .min_h(px(0.))
            .bg(p.bg_canvas)
            .text_color(p.fg_primary)
            .font_family(ui_font)
            .overflow_hidden()
            .on_action(cx.listener(|this, _: &MoveDown, window, cx| this.move_delta(1, window, cx)))
            .on_action(cx.listener(|this, _: &MoveUp, window, cx| this.move_delta(-1, window, cx)))
            .on_action(cx.listener(|this, _: &MoveLeft, window, cx| this.move_file(-1, window, cx)))
            .on_action(cx.listener(|this, _: &MoveRight, window, cx| this.move_file(1, window, cx)))
            .on_action(cx.listener(|this, _: &FocusSidebar, window, cx| this.on_left(window, cx)))
            .on_action(cx.listener(|this, _: &FocusKeys, window, cx| this.on_right(window, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleFavorite, _, cx| this.toggle_favorite_selected(cx)),
            )
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.focus_search(window, cx);
            }))
            .on_action(
                cx.listener(|this, _: &SwitchZone, window, cx| this.switch_zone(true, window, cx)),
            )
            .on_action(cx.listener(|this, _: &SwitchZoneBack, window, cx| {
                this.switch_zone(false, window, cx);
            }))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| {
                this.confirm(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.cancel(window, cx)))
            .on_action(cx.listener(|this, _: &CopyValue, _, cx| this.copy_selected(cx)))
            .on_action(cx.listener(|this, _: &ToggleReveal, _, cx| this.toggle_reveal(cx)))
            .on_action(cx.listener(|this, _: &RevealAll, _, cx| this.toggle_show_all(cx)))
            .on_action(cx.listener(|this, _: &ToggleComment, _, cx| {
                this.toggle_disabled_selected(cx);
            }))
            .on_action(cx.listener(|this, _: &NewKey, window, cx| this.begin_new_key(window, cx)))
            .on_action(cx.listener(|this, _: &RenameKey, window, cx| this.begin_rename(window, cx)))
            .on_action(cx.listener(|this, _: &DeleteKey, window, cx| this.ask_delete(window, cx)))
            .on_action(cx.listener(|this, _: &UndoChange, _, cx| this.undo(cx)))
            .on_action(cx.listener(|this, _: &RedoChange, _, cx| this.redo(cx)))
            .on_action(cx.listener(|this, _: &Save, _, cx| {
                this.save_open(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowShortcuts, window, cx| {
                this.overlay = Overlay::Shortcuts;
                this.focus_self(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Refresh, _, cx| {
                this.begin_scan(false, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleDiff, _, cx| this.toggle_diff(cx)))
            .on_action(cx.listener(|this, _: &CycleDiff, _, cx| this.cycle_diff(cx)))
            .on_action(cx.listener(|this, _: &ToggleEditorView, window, cx| {
                this.toggle_editor_view(window, cx);
            }))
            .on_action(cx.listener(|this, _: &NewFromExample, window, cx| {
                this.begin_create_file(window, cx);
            }))
            .on_action(
                cx.listener(|this, _: &AddFolder, window, cx| this.open_add_folder(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenSettings, window, cx| this.open_settings(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ReloadExternal, _, cx| this.reload_external(cx)))
            .on_action(cx.listener(|this, _: &KeepMine, window, cx| this.cancel(window, cx)))
            .on_action(cx.listener(|this, _: &PasteBlock, _, cx| this.paste_from_clipboard(cx)))
            .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, _| {
                this.focus_project_on_open = false;
            }))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.on_key(event, window, cx);
            }))
            .child(sidebar)
            .child(center);

        if !matches!(self.overlay, Overlay::None) {
            root = root.child(self.overlay_view(p, mono, window, cx));
        }
        if self.tab_menu.is_some() {
            root = root.child(self.tab_menu_layer(p, cx));
        }
        if self.resize_drag.is_some() {
            root = root.child(self.resize_overlay(cx));
        }

        let active_project = self.project_ix.and_then(|ix| self.projects.get(ix));
        let mut title_content = h_flex().flex_1().min_w(px(0.)).items_center().child(
            h_flex()
                .w(px((self.sidebar_width - 12.).max(120.)))
                .flex_shrink_0()
                .gap(px(8.))
                .items_center()
                .child(
                    gpui_kit::img(app_mark())
                        .w(px(16.))
                        .h(px(16.))
                        .rounded(px(4.)),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .line_height(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.fg_primary)
                        .child("Envy"),
                ),
        );
        if let Some(project) = active_project {
            let mut crumb = h_flex()
                .flex_1()
                .min_w(px(0.))
                .pl(px(12.))
                .gap(px(8.))
                .items_center()
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.fg_secondary)
                        .truncate()
                        .child(project.name.clone()),
                );
            if let Some(branch) = display_branch(&project.branch) {
                crumb = crumb.child(
                    h_flex()
                        .gap(px(4.))
                        .items_center()
                        .flex_shrink_0()
                        .child(
                            Icon::new(IconName::GitBranch)
                                .size(px(12.))
                                .text_color(p.fg_tertiary),
                        )
                        .child(
                            div()
                                .text_size(px(11.5))
                                .line_height(px(16.))
                                .text_color(p.fg_tertiary)
                                .child(branch.to_string()),
                        ),
                );
            }
            title_content = title_content.child(crumb);
        }

        if self.focus_project_on_open {
            let typing_keys = self.filter_focused(Zone::Keys, window, cx);
            let typing_projects = self.filter_focused(Zone::Projects, window, cx);
            if typing_keys {
                self.focus_project_on_open = false;
            } else if !typing_projects {
                self.zone = Zone::Projects;
                let handle = self.project_filter.focus_handle(cx);
                if !handle.is_focused(window) {
                    cx.defer_in(window, |this, window, cx| {
                        if !this.focus_project_on_open
                            || this.filter_focused(Zone::Keys, window, cx)
                            || this.filter_focused(Zone::Projects, window, cx)
                        {
                            return;
                        }
                        this.zone = Zone::Projects;
                        let handle = this.project_filter.focus_handle(cx);
                        window.focus(&handle, cx);
                    });
                }
            }
        }

        v_flex()
            .size_full()
            .bg(p.bg_canvas)
            .text_color(p.fg_primary)
            .child(
                TitleBar::new()
                    .h(TITLE_BAR_H)
                    .bg(p.bg_chrome)
                    .border_color(p.border_hairline)
                    .child(title_content),
            )
            .child(root.flex_1().min_h(px(0.)).w_full())
    }
}

impl EnvyApp {
    fn resize_handle(
        id: &'static str,
        kind: ResizeKind,
        p: Palette,
        entity: Entity<Self>,
    ) -> gpui_kit::Div {
        let group = SharedString::from(id);
        let down = entity.clone();
        let up = entity;
        div()
            .group(group.clone())
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(-3.))
            .w(px(7.))
            .cursor_col_resize()
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                cx.stop_propagation();
                let x = event.position.x.as_f32();
                down.update(cx, |this, cx| this.begin_resize(kind, x, cx));
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                up.update(cx, |this, cx| this.end_resize(cx));
            })
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(3.))
                    .w(px(1.))
                    .bg(p.border_hairline)
                    .group_hover(group, move |line| line.bg(p.accent)),
            )
    }

    fn resize_overlay(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let move_entity = cx.entity();
        let up_entity = cx.entity();
        div()
            .absolute()
            .inset_0()
            .occlude()
            .cursor_col_resize()
            .on_mouse_move(move |event, _, cx| {
                let x = event.position.x.as_f32();
                move_entity.update(cx, |this, cx| this.update_resize(x, cx));
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                up_entity.update(cx, |this, cx| this.end_resize(cx));
            })
    }

    fn render_sidebar(
        &self,
        p: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let project_rows = self.projects.clone();
        let visible_projects = self.visible_project_indexes();
        let filter_focused = self.filter_focused(Zone::Projects, window, cx);
        let marked_project = self.marked_project(filter_focused);
        let zone = self.zone;
        let list_focused = zone == Zone::Projects && !filter_focused;
        let entity = cx.entity();

        let count_text = if self.scanning {
            files_found_label(self.scan_found)
        } else if project_rows.is_empty() {
            String::new()
        } else if visible_projects.len() != project_rows.len() {
            format!("{}/{}", visible_projects.len(), project_rows.len())
        } else {
            project_rows.len().to_string()
        };

        let mut header_left = h_flex().gap(px(6.)).items_center().min_w(px(0.)).child(
            div()
                .text_size(px(11.))
                .line_height(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.fg_tertiary)
                .child("PROJECTS"),
        );
        if self.scanning {
            header_left = header_left
                .child(
                    Icon::new(IconName::LoaderCircle)
                        .size(px(12.))
                        .text_color(p.accent),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(14.))
                        .text_color(p.fg_secondary)
                        .child(count_text),
                );
        } else if !count_text.is_empty() {
            header_left = header_left.child(
                div()
                    .px(px(5.))
                    .py(px(1.))
                    .rounded(px(4.))
                    .bg(p.bg_kbd)
                    .text_size(px(10.5))
                    .line_height(px(13.))
                    .text_color(p.fg_tertiary)
                    .child(count_text),
            );
        }

        let header = h_flex()
            .px(px(12.))
            .h(SIDEBAR_HEADER_H)
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .child(header_left)
            .child(
                h_flex()
                    .gap(px(2.))
                    .items_center()
                    .child(
                        toolbar_icon_button(
                            "sb-refresh",
                            IconName::RefreshCw,
                            "Rescan folders",
                            false,
                            false,
                            p,
                        )
                        .tooltip_with_action("Rescan folders", &Refresh, Some("Envy"))
                        .on_click({
                            let entity = entity.clone();
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.begin_scan(false, cx);
                                    cx.notify();
                                });
                            }
                        }),
                    )
                    .child(
                        toolbar_icon_button(
                            "add-folder",
                            IconName::FolderPlus,
                            "Add folder",
                            false,
                            false,
                            p,
                        )
                        .tooltip_with_action("Add folder", &AddFolder, Some("Envy"))
                        .on_click({
                            let entity = entity.clone();
                            move |_, window, cx| {
                                entity.update(cx, |this, cx| this.open_add_folder(window, cx));
                            }
                        }),
                    ),
            );

        let search_inset =
            search_field_inset(SEARCH_BAND_H_PX, SEARCH_FIELD_H_PX, SEARCH_BAND_BORDER_PX);
        let search_box = h_flex()
            .h(px(SEARCH_BAND_H_PX))
            .w_full()
            .px(px(8.))
            .pt(px(search_inset))
            .items_start()
            .flex_shrink_0()
            .child(search_field(
                &self.project_filter,
                "clear-project-filter",
                p,
                window,
                cx,
            ));

        let mut project_list = v_flex()
            .id("project-list")
            .size_full()
            .py(px(4.))
            .gap(px(2.))
            .overflow_y_scroll()
            .track_scroll(&self.project_scroll);

        if self.booting && project_rows.is_empty() {
            project_list = project_list.child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(p.fg_tertiary)
                    .child("Starting"),
            );
        } else if project_rows.is_empty() && !self.scanning {
            project_list = project_list.child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(p.fg_tertiary)
                    .child("No folders added yet."),
            );
        } else if !project_rows.is_empty() && visible_projects.is_empty() {
            project_list = project_list.child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(p.fg_tertiary)
                    .child("No matching projects."),
            );
        }

        let path_budget = sidebar_path_chars(self.sidebar_width.round() as u32);
        let mut last_favorite: Option<bool> = None;
        for (visible_ix, index) in visible_projects.iter().copied().enumerate() {
            let project = &project_rows[index];
            let selected = marked_project == Some(index);
            let favorite = self.config.is_favorite(&project.root);
            if last_favorite != Some(favorite) {
                project_list = project_list.child(sidebar_section(
                    if favorite {
                        "FAVORITES"
                    } else {
                        "ALL PROJECTS"
                    },
                    last_favorite.is_some(),
                    p,
                ));
                last_favorite = Some(favorite);
            }
            let entity = entity.clone();
            let star_entity = entity.clone();
            let group = SharedString::from(format!("project-row-{index}"));
            let native =
                native_project_path(&project.root.display().to_string(), host_path_style());
            let pretty = displayed_project_path(&native);
            let path_label = truncate_path_head(&pretty, path_budget);

            let row_bg = if selected && list_focused {
                Some(p.bg_selected_accent)
            } else if selected {
                Some(p.bg_selected_muted)
            } else {
                None
            };

            let mut row = h_flex()
                .id(SharedString::from(format!("project-{visible_ix}")))
                .group(group.clone())
                .mx(px(8.))
                .px(px(8.))
                .h(SIDEBAR_ROW_BRANCH_H)
                .flex_shrink_0()
                .gap(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(if selected && list_focused {
                    p.accent_ring
                } else {
                    p.bg_chrome.opacity(0.)
                })
                .items_center()
                .overflow_hidden()
                .cursor_pointer()
                .tooltip(hover_tip(native))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    entity.update(cx, |this, cx| {
                        this.zone = Zone::Projects;
                        this.remember_project(index, cx);
                        this.select_project(index, false, window, cx);
                    });
                });

            if let Some(bg) = row_bg {
                row = row.bg(bg);
            } else {
                row = row.hover(move |style| style.bg(p.bg_hover));
            }

            let text_col = v_flex()
                .flex_1()
                .min_w(px(0.))
                .justify_center()
                .gap(px(1.))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .font_weight(if selected {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .truncate()
                        .text_color(if selected {
                            p.fg_primary
                        } else {
                            p.fg_primary.opacity(0.92)
                        })
                        .child(project.name.clone()),
                )
                .child(
                    div()
                        .min_w(px(0.))
                        .text_size(px(11.))
                        .line_height(px(14.))
                        .text_color(p.fg_tertiary)
                        .truncate()
                        .child(path_label),
                );

            row = row.child(text_col);

            let mut star = div()
                .w(px(24.))
                .h(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0();
            if !favorite {
                star = star.invisible().group_hover(group, |style| style.visible());
            }
            row = row.child(
                star.child(
                    row_icon_button(
                        SharedString::from(format!("pin-{index}")),
                        if favorite {
                            IconName::StarFill
                        } else {
                            IconName::Star
                        },
                        if favorite {
                            "Unpin project"
                        } else {
                            "Pin project"
                        },
                    )
                    .text_color(if favorite { p.accent } else { p.fg_tertiary })
                    .on_click(move |_, _, cx| {
                        star_entity.update(cx, |this, cx| {
                            this.project_ix = Some(index);
                            this.toggle_favorite_selected(cx);
                        });
                    }),
                ),
            );

            project_list = project_list.child(row);
        }

        v_flex()
            .w_full()
            .h_full()
            .bg(p.bg_chrome)
            .text_color(p.fg_primary)
            .border_r_1()
            .border_color(p.border_hairline)
            .child(header)
            .child(search_box)
            .child(
                div()
                    .id("project-scroll-host")
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .overflow_hidden()
                    .child(project_list)
                    .child(Scrollbar::vertical(&self.project_scroll).mode(ScrollbarMode::Hover)),
            )
    }

    fn render_tabs(
        &self,
        p: Palette,
        mono: SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let file_tabs = self
            .project_ix
            .and_then(|ix| self.projects.get(ix))
            .map(|project| project.files.clone())
            .unwrap_or_default();
        let file_ix = self.file_ix;
        let zone = self.zone;
        let can_compare = file_tabs.len() >= 2;
        let labels = self.current_tab_labels();

        let mut tabs = h_flex()
            .id("file-tabs")
            .w_full()
            .h(TAB_BAR_H)
            .flex_shrink_0()
            .px(px(8.))
            .gap(px(2.))
            .items_end()
            .bg(p.bg_chrome)
            .border_b_1()
            .border_color(p.border_hairline)
            .overflow_x_scroll();

        for (index, file) in file_tabs.iter().enumerate() {
            let is_left = self.diff_open && self.diff_left == Some(index);
            let is_right = self.diff_open && self.diff_right == Some(index);
            let is_active = if self.diff_open {
                is_left || is_right
            } else {
                file_ix == Some(index)
            };
            let cursor_tab = zone == Zone::Tabs && self.tab_cursor == index;
            let entity = cx.entity();
            let name = file.name.clone();
            let label = labels.get(index);
            let suffix = label.map(|label| label.suffix.clone()).unwrap_or_default();
            let relative = label
                .map(|label| label.relative.clone())
                .unwrap_or_else(|| name.clone());
            if let FilePrompt::Rename { file_ix, input, .. } = &self.file_prompt {
                if *file_ix == index {
                    let input = input.clone();
                    tabs = tabs.child(editing_tab(&input, file.prod, p, mono.clone()));
                    continue;
                }
            }

            let mut tip_parts = vec![relative];
            if file.prod {
                tip_parts.push("Production".into());
            }
            if file.missing_vs_example {
                tip_parts.push("Missing keys vs .env.example".into());
            }
            let unsaved = self.open.as_ref().is_some_and(|open| {
                open.path == file.path
                    && (open.is_dirty()
                        || self.save_failed.as_ref().is_some_and(|failed| {
                            failed.path == file.path && self.save_phase == SavePhase::Failed
                        }))
            });
            if unsaved {
                tip_parts.push("Unsaved".into());
            }
            if can_compare && !self.diff_open && file_ix != Some(index) {
                tip_parts.push("Ctrl+click to compare".into());
            }
            let tip = tip_parts.join(" · ");

            let mut tab = h_flex()
                .id(SharedString::from(format!("file-{index}")))
                .relative()
                .h(px(TAB_H_PX))
                .px(px(TAB_PAD_X_PX))
                .gap(px(TAB_GAP_PX))
                .items_center()
                .rounded_t(px(6.))
                .cursor_pointer()
                .font_family(mono.clone())
                .text_size(px(TAB_TEXT_PX))
                .line_height(px(TAB_LINE_PX))
                .font_weight(if is_active {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if is_active || cursor_tab {
                    p.fg_primary
                } else {
                    p.fg_secondary
                })
                .tooltip(hover_tip(tip))
                .on_mouse_down(MouseButton::Left, {
                    let entity = entity.clone();
                    move |event, window, cx| {
                        entity.update(cx, |this, cx| {
                            this.zone = Zone::Tabs;
                            this.tab_cursor = index;
                            if event.click_count >= 2
                                && !event.modifiers.control
                                && !event.modifiers.shift
                            {
                                this.begin_rename_file(index, window, cx);
                            } else if event.modifiers.control || event.modifiers.shift {
                                this.toggle_compare_tab(index, cx);
                            } else {
                                this.activate_tab(index, false, window, cx);
                            }
                        });
                    }
                })
                .on_mouse_down(MouseButton::Right, move |_, window, cx| {
                    cx.stop_propagation();
                    entity.update(cx, |this, cx| this.open_tab_menu(index, window, cx));
                });

            if cursor_tab {
                tab = tab
                    .bg(p.accent_subtle)
                    .border_1()
                    .border_color(p.accent_ring);
            } else if is_active {
                tab = tab
                    .bg(p.bg_canvas)
                    .border_t_1()
                    .border_l_1()
                    .border_r_1()
                    .border_color(p.border_hairline);
            } else {
                tab = tab.hover(move |style| style.bg(p.bg_hover).text_color(p.fg_primary));
            }

            if is_left || is_right {
                let side_tag = if is_left { "L" } else { "R" };
                tab = tab.child(
                    div()
                        .px(px(4.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .bg(p.accent_subtle)
                        .text_size(px(10.))
                        .line_height(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.accent)
                        .child(side_tag),
                );
            }

            if file.prod {
                tab = tab.child(
                    div()
                        .size(px(TAB_DOT_PX))
                        .rounded(px(999.))
                        .bg(p.prod_dot)
                        .flex_shrink_0(),
                );
            }

            tab = tab.child(file_title(&name, &suffix, p));

            if unsaved {
                tab = tab.child(
                    div()
                        .size(px(TAB_DOT_PX))
                        .rounded(px(999.))
                        .bg(p.accent)
                        .flex_shrink_0(),
                );
            }

            if is_active {
                tab = tab.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left(px(6.))
                        .right(px(6.))
                        .h(px(2.))
                        .rounded(px(2.))
                        .bg(p.accent),
                );
            }

            tabs = tabs.child(tab);
        }

        if let FilePrompt::Create { input, .. } = &self.file_prompt {
            let input = input.clone();
            let name = input.read(cx).value().to_string();
            tabs = tabs.child(editing_tab(
                &input,
                is_prod_filename(&name),
                p,
                mono.clone(),
            ));
        }

        let error = match &self.file_prompt {
            FilePrompt::Create { error, .. } | FilePrompt::Rename { error, .. } => error.clone(),
            FilePrompt::None => None,
        };
        let mut column = v_flex().w_full().flex_shrink_0().child(tabs);
        if matches!(self.file_prompt, FilePrompt::Create { .. }) {
            column = column.child(self.render_new_file_sources(p, cx));
        }
        if let Some(error) = error {
            column = column.child(
                div()
                    .px(px(12.))
                    .pb(px(6.))
                    .text_size(px(11.5))
                    .line_height(px(15.))
                    .text_color(p.warning)
                    .child(error),
            );
        }
        column
    }

    fn render_toolbar(
        &self,
        p: Palette,
        missing_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let entity = cx.entity();
        let show_all = self.show_all;
        let diff_open = self.diff_open;
        let text_mode = !diff_open && self.editor_view == EditorView::Text;
        let can_undo = self.open.as_ref().is_some_and(|open| open.can_undo());
        let can_redo = self.open.as_ref().is_some_and(|open| open.can_redo());
        let has_open = self.open.is_some();
        let file_count = self.file_count();

        let left_side = if diff_open {
            let mut left = h_flex()
                .items_center()
                .gap(px(8.))
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden();
            left = left.child(
                div().flex_shrink_0().child(
                    toolbar_icon_button(
                        "tb-diff-close",
                        IconName::X,
                        "Close compare · Esc",
                        false,
                        false,
                        p,
                    )
                    .on_click({
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.clear_compare();
                                this.focus_self(window, cx);
                                cx.notify();
                            });
                        }
                    }),
                ),
            );
            if let Some(report) = &self.diff_report {
                let mut different = 0usize;
                let mut only_left = 0usize;
                let mut only_right = 0usize;
                let mut disabled = 0usize;
                for row in &report.rows {
                    match row.kind {
                        diff::DiffKind::Different => different += 1,
                        diff::DiffKind::OnlyLeft => only_left += 1,
                        diff::DiffKind::OnlyRight => only_right += 1,
                        diff::DiffKind::Disabled => disabled += 1,
                        diff::DiffKind::Same => {}
                    }
                }
                if different == 0 && only_left == 0 && only_right == 0 && disabled == 0 {
                    left = left.child(neutral_summary_chip(
                        "diff-match",
                        IconName::Check,
                        p.fg_secondary,
                        "Files match".into(),
                        p,
                    ));
                } else {
                    if different > 0 {
                        left = left.child(neutral_summary_chip(
                            "diff-differ",
                            IconName::EqualNot,
                            p.warning,
                            format!("{different} differ"),
                            p,
                        ));
                    }
                    if only_left > 0 {
                        left = left.child(neutral_summary_chip(
                            "diff-left",
                            IconName::Plus,
                            p.accent,
                            format!("{only_left} left only"),
                            p,
                        ));
                    }
                    if only_right > 0 {
                        left = left.child(neutral_summary_chip(
                            "diff-right",
                            IconName::Minus,
                            p.warning,
                            format!("{only_right} right only"),
                            p,
                        ));
                    }
                    if disabled > 0 {
                        left = left.child(neutral_summary_chip(
                            "diff-off",
                            IconName::ToggleLeft,
                            p.fg_tertiary,
                            format!("{disabled} off"),
                            p,
                        ));
                    }
                }
            }
            if file_count >= 3 {
                left = left.child(
                    div().flex_shrink_0().child(
                        toolbar_icon_button(
                            "tb-diff-cycle",
                            IconName::ArrowLeftRight,
                            "Compare another file",
                            false,
                            false,
                            p,
                        )
                        .tooltip_with_action("Compare another file", &CycleDiff, Some("Envy"))
                        .on_click({
                            let entity = entity.clone();
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| this.cycle_diff(cx));
                            }
                        }),
                    ),
                );
            }
            left
        } else if text_mode {
            let line_count = self
                .open
                .as_ref()
                .map(|open| open.document.nodes.len())
                .unwrap_or(0);
            let line_label = if line_count == 1 {
                "1 line".to_string()
            } else {
                format!("{line_count} lines")
            };
            h_flex()
                .items_center()
                .gap(px(8.))
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .child(toolbar_chip(
                    "plain-text-notice",
                    IconName::Eye,
                    p.fg_tertiary,
                    "Plain text · values visible".into(),
                    p.bg_kbd,
                    p.border_hairline,
                    p.fg_secondary,
                ))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(p.fg_tertiary)
                        .whitespace_nowrap()
                        .child(line_label),
                )
        } else {
            let total = self.entry_nodes().len();
            let shown = self.visible_entry_nodes().len();
            let count_label = if !self.key_query.trim().is_empty() {
                format!("{shown} of {total}")
            } else if total == 1 {
                "1 key".to_string()
            } else {
                format!("{total} keys")
            };

            let mut left = h_flex()
                .items_center()
                .gap(px(8.))
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .child(
                    div()
                        .w(px(KEY_SEARCH_PREFERRED_PX))
                        .min_w(px(KEY_SEARCH_MIN_PX))
                        .flex_shrink(TOOLBAR_SEARCH_SHRINK)
                        .child(search_field(
                            &self.key_filter,
                            "clear-key-filter",
                            p,
                            window,
                            cx,
                        )),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(p.fg_tertiary)
                        .whitespace_nowrap()
                        .child(count_label),
                );

            if missing_count > 0 {
                let compare_entity = entity.clone();
                let label = format!("{missing_count} missing vs .env.example");
                left = left.child(
                    toolbar_chip(
                        "missing-vs-example-pill",
                        IconName::Info,
                        p.warning,
                        label,
                        p.warning_bg,
                        p.warning_border,
                        p.warning,
                    )
                    .cursor_pointer()
                    .hover(move |style| style.bg(p.warning.opacity(0.18)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        compare_entity.update(cx, |this, cx| this.compare_with_example(cx));
                    }),
                );
            }

            left
        };

        let right_side = h_flex()
            .items_center()
            .gap(px(4.))
            .flex_shrink_0()
            .child(
                copy_icon_button(
                    TOOLBAR_COPY_ID,
                    "Copy all",
                    CopyButtonKind::Toolbar,
                    self.copy_ack(TOOLBAR_COPY_ID),
                    !has_open,
                    cx.reduce_motion(),
                    p,
                )
                .on_click({
                    let entity = entity.clone();
                    move |_, _, cx| {
                        entity.update(cx, |this, cx| this.copy_all(cx));
                    }
                }),
            )
            .child(
                toolbar_icon_button(
                    "tb-new",
                    IconName::Plus,
                    "New file",
                    false,
                    self.project_ix.is_none(),
                    p,
                )
                .tooltip_with_action("New file", &NewFromExample, Some("Envy"))
                .on_click({
                    let entity = entity.clone();
                    move |_, window, cx| {
                        entity.update(cx, |this, cx| this.begin_create_file(window, cx));
                    }
                }),
            )
            .child(
                toolbar_icon_button("tb-undo", IconName::Undo2, "Undo", false, !can_undo, p)
                    .tooltip_with_action("Undo", &UndoChange, Some("Envy"))
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| this.undo(cx));
                        }
                    }),
            )
            .child(
                toolbar_icon_button("tb-redo", IconName::Redo2, "Redo", false, !can_redo, p)
                    .tooltip_with_action("Redo", &RedoChange, Some("Envy"))
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| this.redo(cx));
                        }
                    }),
            )
            .child(v_divider(p))
            .child(
                toolbar_icon_button(
                    "tb-eye",
                    if show_all {
                        IconName::EyeOff
                    } else {
                        IconName::Eye
                    },
                    if show_all {
                        "Hide all values"
                    } else {
                        "Show all values"
                    },
                    show_all && !text_mode,
                    text_mode,
                    p,
                )
                .tooltip_with_action(
                    if show_all {
                        "Hide all values"
                    } else {
                        "Show all values"
                    },
                    &RevealAll,
                    Some("Envy"),
                )
                .on_click({
                    let entity = entity.clone();
                    move |_, _, cx| {
                        entity.update(cx, |this, cx| this.toggle_show_all(cx));
                    }
                }),
            )
            .child(
                toolbar_icon_button(
                    "tb-view",
                    if text_mode {
                        IconName::List
                    } else {
                        IconName::FileCode
                    },
                    if text_mode {
                        "Table view"
                    } else {
                        "Plain text view"
                    },
                    text_mode,
                    !has_open,
                    p,
                )
                .tooltip_with_action(
                    if text_mode {
                        "Table view"
                    } else {
                        "Plain text view"
                    },
                    &ToggleEditorView,
                    Some("Envy"),
                )
                .on_click({
                    let entity = entity.clone();
                    move |_, window, cx| {
                        entity.update(cx, |this, cx| this.toggle_editor_view(window, cx));
                    }
                }),
            )
            .child(
                toolbar_icon_button(
                    "tb-diff",
                    IconName::Columns2,
                    if diff_open {
                        "Close compare"
                    } else {
                        "Compare two files"
                    },
                    diff_open,
                    file_count < 2,
                    p,
                )
                .tooltip_with_action("Compare two files", &ToggleDiff, Some("Envy"))
                .on_click({
                    let entity = entity.clone();
                    move |_, _, cx| {
                        entity.update(cx, |this, cx| this.toggle_diff(cx));
                    }
                }),
            )
            .child(v_divider(p))
            .child(
                toolbar_icon_button(
                    "tb-shortcuts",
                    IconName::Keyboard,
                    "Keyboard shortcuts",
                    false,
                    false,
                    p,
                )
                .tooltip_with_action("Keyboard shortcuts", &ShowShortcuts, Some("Envy"))
                .on_click({
                    let entity = entity.clone();
                    move |_, window, cx| {
                        entity.update(cx, |this, cx| {
                            this.overlay = Overlay::Shortcuts;
                            this.focus_self(window, cx);
                            cx.notify();
                        });
                    }
                }),
            )
            .child(
                toolbar_icon_button(
                    "tb-settings",
                    IconName::Settings,
                    "Settings",
                    false,
                    false,
                    p,
                )
                .tooltip_with_action("Settings", &OpenSettings, Some("Envy"))
                .on_click({
                    let entity = entity.clone();
                    move |_, window, cx| {
                        entity.update(cx, |this, cx| this.open_settings(window, cx));
                    }
                }),
            );

        let search_inset =
            search_field_inset(SEARCH_BAND_H_PX, SEARCH_FIELD_H_PX, SEARCH_BAND_BORDER_PX);
        h_flex()
            .h(TOOLBAR_H)
            .flex_shrink_0()
            .px(px(12.))
            .pt(px(search_inset))
            .gap(px(12.))
            .items_start()
            .justify_between()
            .overflow_hidden()
            .bg(p.bg_canvas)
            .border_b_1()
            .border_color(p.border_hairline)
            .child(left_side)
            .child(right_side)
    }

    fn render_key_table(
        &self,
        p: Palette,
        mono: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let visible_layout = self.visible_layout();
        let cursor = self.cursor;
        let show_all = self.show_all;
        let revealed = self.revealed.clone();
        let keys_focused = self.zone == Zone::Keys && !self.filter_focused(Zone::Keys, window, cx);
        let editing_node = self.editor.as_ref().and_then(|editor| editor.node);
        let editing_new = self
            .editor
            .as_ref()
            .is_some_and(|editor| matches!(editor.kind, EditKind::New));
        let query = self.key_query.trim().to_ascii_lowercase();
        let query_active = !query.is_empty();
        let entity = cx.entity();
        let reduce_motion = cx.reduce_motion();

        let has_any_entries = !visible_layout.entry_nodes.is_empty();
        let key_w = px(self.effective_key_column());

        let col_header = h_flex()
            .h(COL_HEADER_H)
            .flex_shrink_0()
            .px(px(16.))
            .items_center()
            .bg(p.bg_chrome.opacity(0.55))
            .border_b_1()
            .border_color(p.border_hairline)
            .child(
                div()
                    .relative()
                    .w(key_w)
                    .flex_shrink_0()
                    .pl(px(30.))
                    .pr(px(16.))
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.fg_tertiary)
                    .child("KEY")
                    .child(Self::resize_handle(
                        "key-col-resize",
                        ResizeKind::KeyColumn,
                        p,
                        entity.clone(),
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.fg_tertiary)
                    .child("VALUE"),
            )
            .child(div().w(KEY_ACTIONS_W).flex_shrink_0());

        let mut table = v_flex()
            .id("key-table")
            .size_full()
            .py(px(4.))
            .gap(px(1.))
            .overflow_y_scroll()
            .track_scroll(&self.key_scroll);

        if !has_any_entries && !editing_new && self.open.is_some() {
            let note = if query_active {
                "No matching keys."
            } else {
                "No keys in this file. Press N to add one."
            };
            table = table.child(
                div()
                    .px(px(16.))
                    .py(px(24.))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(p.fg_tertiary)
                    .child(note),
            );
        }

        let mut entry_row_ix = 0usize;
        for (item_ix, item) in visible_layout.items.iter().enumerate() {
            match item {
                LayoutItem::Section { title, .. } => {
                    let divider = h_flex()
                        .id(SharedString::from(format!("section-{item_ix}")))
                        .h(SECTION_ROW_H)
                        .flex_shrink_0()
                        .mx(px(16.))
                        .when(item_ix > 0, |row| row.mt(px(8.)))
                        .items_center()
                        .gap(px(10.))
                        .tooltip(hover_tip(title.clone()))
                        .child(
                            div()
                                .max_w(px(360.))
                                .text_size(px(11.))
                                .line_height(px(14.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.fg_tertiary)
                                .truncate()
                                .child(title.clone()),
                        )
                        .child(div().flex_1().h(px(1.)).bg(p.border_hairline));
                    table = table.child(divider);
                }
                LayoutItem::Entry {
                    node_index,
                    description,
                    related_to_active,
                    ..
                } => {
                    let Some(open) = self.open.as_ref() else {
                        continue;
                    };
                    let Some(Node::Entry(entry)) = open.document.nodes.get(*node_index) else {
                        continue;
                    };
                    let row_ix = entry_row_ix;
                    entry_row_ix += 1;

                    let node = *node_index;
                    if editing_new
                        && self.editor.as_ref().and_then(|editor| editor.preview_node) == Some(node)
                    {
                        continue;
                    }
                    let key = entry.key.clone();
                    let value = entry.value.clone();
                    let exported = entry.exported;
                    let disabled = entry.disabled;
                    let related = *related_to_active;
                    let description = description.clone();

                    let selected = cursor == row_ix;
                    let visible = show_all || revealed.contains(&key);
                    let click_entity = entity.clone();
                    let toggle_entity = entity.clone();
                    let copy_entity = entity.clone();
                    let reveal_entity = entity.clone();
                    let key_group = SharedString::from(format!("key-row-{node}"));

                    let show_value_input = editing_node == Some(node)
                        && self
                            .editor
                            .as_ref()
                            .is_some_and(|editor| matches!(editor.kind, EditKind::Value));
                    let show_rename_input = editing_node == Some(node)
                        && self
                            .editor
                            .as_ref()
                            .is_some_and(|editor| matches!(editor.kind, EditKind::Rename));
                    let editing_this = show_value_input || show_rename_input;

                    let row_bg = if editing_this || (selected && keys_focused) {
                        Some(p.bg_selected_accent)
                    } else if selected {
                        Some(p.bg_selected_muted)
                    } else {
                        None
                    };

                    let mut row = h_flex()
                        .id(SharedString::from(format!("key-{node}")))
                        .group(key_group.clone())
                        .mx(px(8.))
                        .px(px(7.))
                        .h(KEY_ROW_H)
                        .flex_shrink_0()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(if selected && keys_focused && !editing_this {
                            p.accent_ring
                        } else {
                            p.bg_canvas.opacity(0.)
                        })
                        .items_center()
                        .overflow_hidden()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            click_entity.update(cx, |this, cx| {
                                this.zone = Zone::Keys;
                                let editing_this = this
                                    .editor
                                    .as_ref()
                                    .is_some_and(|editor| editor.node == Some(node));
                                if editing_this {
                                    this.cursor = row_ix;
                                } else {
                                    if this.editor.is_some() {
                                        this.commit_editor(None, false, window, cx);
                                    }
                                    this.cursor = row_ix;
                                    this.focus_self(window, cx);
                                }
                                cx.notify();
                            });
                        });

                    if let Some(bg) = row_bg {
                        row = row.bg(bg);
                    } else {
                        row = row.hover(move |style| style.bg(p.bg_hover));
                    }

                    let key_hit = query_active && fuzzy_hit(&key, &query);
                    let desc_hit = query_active
                        && description.as_deref().is_some_and(|d| fuzzy_hit(d, &query));
                    let key_color = if disabled {
                        if key_hit || desc_hit {
                            p.accent.opacity(0.75)
                        } else {
                            p.fg_tertiary
                        }
                    } else if key_hit || desc_hit {
                        p.accent
                    } else {
                        p.fg_primary
                    };

                    let mut full_key_tip = if exported {
                        format!("export {key}")
                    } else {
                        key.clone()
                    };
                    if disabled {
                        full_key_tip.push_str(" (off)");
                    }
                    if let Some(desc) = description {
                        full_key_tip.push('\n');
                        full_key_tip.push_str(&desc);
                    }

                    let mut toggle_btn = div()
                        .id(SharedString::from(format!("toggle-{node}")))
                        .w(px(KEY_TOGGLE_W_PX))
                        .h(px(KEY_TOGGLE_W_PX))
                        .flex_shrink_0()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(p.bg_canvas.opacity(0.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .tooltip(hover_tip(if disabled {
                            "Turn on · Ctrl+Space"
                        } else {
                            "Turn off · Ctrl+Space"
                        }))
                        .hover(move |style| style.bg(p.accent_subtle).border_color(p.accent_ring))
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            cx.stop_propagation();
                            toggle_entity.update(cx, |this, cx| {
                                this.zone = Zone::Keys;
                                this.focus_self(window, cx);
                                this.toggle_disabled_node(node, cx);
                            });
                        })
                        .child(
                            Icon::new(if disabled {
                                IconName::ToggleLeft
                            } else {
                                IconName::ToggleRight
                            })
                            .size(px(15.))
                            .text_color(if disabled {
                                p.fg_secondary
                            } else {
                                p.accent
                            }),
                        );
                    if !disabled && !(selected && keys_focused) {
                        toggle_btn = toggle_btn
                            .invisible()
                            .group_hover(key_group.clone(), |style| style.visible());
                    }

                    let key_edit = entity.clone();
                    let mut key_line = h_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .items_center()
                        .font_family(mono.clone())
                        .text_size(px(KEY_TEXT_PX))
                        .line_height(px(KEY_LINE_PX))
                        .text_color(key_color);
                    if exported {
                        key_line = key_line.child(
                            div()
                                .flex_shrink_0()
                                .text_color(p.fg_tertiary)
                                .child("export "),
                        );
                    }
                    if show_rename_input {
                        if let Some(key_input) = self.editor.as_ref().and_then(|e| e.key.as_ref()) {
                            key_line =
                                key_line.child(div().flex_1().min_w(px(0.)).child(inline_field(
                                    key_input,
                                    p,
                                    KEY_LINE_PX,
                                )));
                        }
                    } else {
                        key_line = key_line.child(
                            div()
                                .id(SharedString::from(format!("key-text-{node}")))
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .cursor_text()
                                .tooltip(hover_tip(full_key_tip))
                                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                    cx.stop_propagation();
                                    key_edit.update(cx, |this, cx| {
                                        this.edit_entry_at(row_ix, RowTarget::KeyName, window, cx);
                                    });
                                })
                                .child(key.clone()),
                        );
                    }
                    let mut key_cell = h_flex()
                        .id(SharedString::from(format!("key-name-{node}")))
                        .w(key_w)
                        .flex_shrink_0()
                        .pr(px(KEY_CELL_PAD_RIGHT_PX))
                        .min_w(px(0.))
                        .gap(px(KEY_CELL_GAP_PX))
                        .items_center();
                    if related {
                        key_cell = key_cell.pl(px(KEY_RELATED_PAD_PX)).child(
                            Icon::new(IconName::CornerDownRight)
                                .size(px(KEY_RELATED_ICON_PX))
                                .text_color(p.fg_tertiary.opacity(0.65)),
                        );
                    }
                    let key_cell = key_cell
                        .child(toggle_btn)
                        .child(key_line)
                        .into_any_element();

                    let shown = display_value(&value, visible);
                    let dim_empty = value.is_empty();
                    let value_color = if disabled {
                        if dim_empty {
                            p.fg_tertiary.opacity(0.55)
                        } else if visible {
                            p.fg_tertiary
                        } else {
                            p.fg_tertiary.opacity(0.75)
                        }
                    } else if dim_empty {
                        p.fg_tertiary
                    } else if visible {
                        p.fg_primary
                    } else {
                        p.fg_secondary
                    };
                    let value_tip = if visible && !dim_empty && !show_value_input {
                        value.clone()
                    } else {
                        String::new()
                    };
                    let value_edit = entity.clone();
                    let mut value_cell = div()
                        .id(SharedString::from(format!("key-value-{node}")))
                        .flex_1()
                        .min_w(px(0.))
                        .pr(px(VALUE_PAD_RIGHT_PX))
                        .font_family(mono.clone())
                        .text_size(px(KEY_TEXT_PX))
                        .line_height(px(KEY_LINE_PX))
                        .text_color(value_color);
                    if show_value_input {
                        if let Some(editor) = &self.editor {
                            value_cell =
                                value_cell.child(inline_field(&editor.value, p, KEY_LINE_PX));
                        }
                    } else {
                        value_cell = value_cell
                            .truncate()
                            .cursor_text()
                            .when(!value_tip.is_empty(), |cell| {
                                cell.tooltip(hover_tip(value_tip))
                            })
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                value_edit.update(cx, |this, cx| {
                                    this.edit_entry_at(row_ix, RowTarget::Value, window, cx);
                                });
                            })
                            .child(shown);
                    }
                    let value_cell = value_cell.into_any_element();

                    let mut actions = h_flex()
                        .w(KEY_ACTIONS_W)
                        .gap(px(4.))
                        .flex_shrink_0()
                        .items_center()
                        .justify_end();
                    if !selected || !keys_focused {
                        actions = actions
                            .invisible()
                            .group_hover(key_group, |style| style.visible());
                    }
                    if !editing_this {
                        let copy_id = row_copy_id(node);
                        actions = actions
                            .child(
                                copy_icon_button(
                                    copy_id.clone(),
                                    "Copy value · C",
                                    CopyButtonKind::Row,
                                    self.copy_ack(&copy_id),
                                    false,
                                    reduce_motion,
                                    p,
                                )
                                .on_click(move |_, _, cx| {
                                    copy_entity.update(cx, |this, cx| {
                                        this.zone = Zone::Keys;
                                        this.cursor = row_ix;
                                        this.copy_selected(cx);
                                    });
                                }),
                            )
                            .child(
                                row_icon_button(
                                    SharedString::from(format!("eye-{node}")),
                                    if visible {
                                        IconName::EyeOff
                                    } else {
                                        IconName::Eye
                                    },
                                    if visible {
                                        "Hide value · Space"
                                    } else {
                                        "Show value · Space"
                                    },
                                )
                                .text_color(if visible { p.accent } else { p.fg_secondary })
                                .on_click(move |_, _, cx| {
                                    reveal_entity.update(cx, |this, cx| {
                                        this.zone = Zone::Keys;
                                        this.cursor = row_ix;
                                        this.toggle_reveal(cx);
                                    });
                                }),
                            );
                    }

                    row = row.child(key_cell).child(value_cell).child(actions);
                    table = table.child(row);
                    if show_rename_input {
                        if let Some(dropdown) =
                            self.render_suggestions_dropdown(p, mono.clone(), cx)
                        {
                            table = table.child(dropdown);
                        }
                    }
                }
            }
        }

        if editing_new {
            if let Some(editor) = &self.editor {
                let mut row = h_flex()
                    .mx(px(8.))
                    .mt(px(4.))
                    .px(px(7.))
                    .h(KEY_ROW_H)
                    .flex_shrink_0()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.bg_canvas.opacity(0.))
                    .items_center()
                    .bg(p.bg_selected_accent);
                if let Some(key_input) = &editor.key {
                    row = row.child(
                        h_flex()
                            .w(key_w)
                            .flex_shrink_0()
                            .pr(px(KEY_CELL_PAD_RIGHT_PX))
                            .gap(px(KEY_CELL_GAP_PX))
                            .items_center()
                            .font_family(mono.clone())
                            .text_size(px(KEY_TEXT_PX))
                            .line_height(px(KEY_LINE_PX))
                            .text_color(p.fg_primary)
                            .child(div().w(px(KEY_TOGGLE_W_PX)).flex_shrink_0())
                            .child(div().flex_1().min_w(px(0.)).child(inline_field(
                                key_input,
                                p,
                                KEY_LINE_PX,
                            ))),
                    );
                }
                row = row
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .pr(px(VALUE_PAD_RIGHT_PX))
                            .font_family(mono.clone())
                            .text_size(px(KEY_TEXT_PX))
                            .line_height(px(KEY_LINE_PX))
                            .text_color(p.fg_primary)
                            .child(inline_field(&editor.value, p, KEY_LINE_PX)),
                    )
                    .child(
                        div()
                            .w(KEY_ACTIONS_W)
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .child(kbd_chip("Enter", p, mono.clone())),
                    );
                table = table.child(row);
                if let Some(dropdown) = self.render_suggestions_dropdown(p, mono.clone(), cx) {
                    table = table.child(dropdown);
                }
            }
        } else if self.open.is_some() && (has_any_entries || !query_active) {
            let add_entity = entity.clone();
            table = table.child(
                h_flex()
                    .id("inline-add-key")
                    .mx(px(8.))
                    .mt(px(4.))
                    .px(px(8.))
                    .h(KEY_ROW_H)
                    .flex_shrink_0()
                    .gap(px(6.))
                    .rounded(px(6.))
                    .items_center()
                    .text_color(p.fg_tertiary)
                    .cursor_pointer()
                    .hover(move |style| style.bg(p.bg_hover).text_color(p.fg_secondary))
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        add_entity.update(cx, |this, cx| this.begin_new_key(window, cx));
                    })
                    .child(Icon::new(IconName::Plus).size(px(13.)))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .child("Add key"),
                    ),
            );
        }

        let mut pane = v_flex().flex_1().min_h(px(0.)).w_full().overflow_hidden();

        if self.open.as_ref().is_some_and(|open| open.external_change) {
            let reload_entity = entity.clone();
            let keep_entity = entity.clone();
            pane = pane.child(
                h_flex()
                    .h(px(36.))
                    .flex_shrink_0()
                    .px(px(12.))
                    .gap(px(8.))
                    .items_center()
                    .justify_between()
                    .bg(p.warning_bg)
                    .border_b_1()
                    .border_color(p.warning_border)
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                Icon::new(IconName::TriangleAlert)
                                    .size(px(14.))
                                    .text_color(p.warning),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(p.fg_primary)
                                    .child("This file changed on disk."),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .items_center()
                            .child(
                                Button::new("ext-reload")
                                    .outline()
                                    .xsmall()
                                    .label("Reload")
                                    .on_click(move |_, _, cx| {
                                        reload_entity
                                            .update(cx, |this, cx| this.reload_external(cx));
                                    }),
                            )
                            .child(
                                Button::new("ext-keep")
                                    .ghost()
                                    .xsmall()
                                    .label("Keep mine")
                                    .on_click(move |_, window, cx| {
                                        keep_entity.update(cx, |this, cx| this.cancel(window, cx));
                                    }),
                            ),
                    ),
            );
        }

        pane.child(col_header).child(
            div()
                .id("key-scroll-host")
                .relative()
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .overflow_hidden()
                .child(table)
                .child(Scrollbar::vertical(&self.key_scroll).mode(ScrollbarMode::Hover)),
        )
    }

    fn render_suggestions_dropdown(
        &self,
        p: Palette,
        mono: SharedString,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::Div> {
        let editor = self.editor.as_ref()?;
        if !editor.suggestions_open || editor.suggestions.is_empty() {
            return None;
        }
        let query = editor
            .key
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        let entity = cx.entity();

        let mut rows = v_flex().p(px(3.)).gap(px(1.));
        for (ix, suggestion) in editor.suggestions.iter().enumerate() {
            let pick_entity = entity.clone();
            let rest_color = p.fg_tertiary;

            let key_label = if let Some((start, end)) = match_span(&suggestion.key, &query) {
                let before = suggestion.key[..start].to_string();
                let matched = suggestion.key[start..end].to_string();
                let after = suggestion.key[end..].to_string();
                h_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .font_family(mono.clone())
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .when(!before.is_empty(), |row| {
                        row.child(div().text_color(rest_color).child(before))
                    })
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.fg_primary)
                            .child(matched),
                    )
                    .when(!after.is_empty(), |row| {
                        row.child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_color(rest_color)
                                .child(after),
                        )
                    })
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .font_family(mono.clone())
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .truncate()
                    .text_color(p.fg_secondary)
                    .child(suggestion.key.clone())
                    .into_any_element()
            };

            let item = h_flex()
                .id(SharedString::from(format!("key-suggest-{ix}")))
                .h(px(24.))
                .px(px(8.))
                .rounded(px(4.))
                .items_center()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    pick_entity.update(cx, |this, cx| {
                        this.accept_suggestion(ix, window, cx);
                    });
                })
                .child(key_label)
                .hover(move |style| style.bg(p.bg_hover));
            rows = rows.child(item);
        }

        let list = v_flex()
            .ml(px(16.))
            .mt(px(3.))
            .mb(px(4.))
            .w(px(self.effective_key_column()) - px(12.))
            .flex_shrink_0()
            .rounded(px(6.))
            .bg(p.bg_chrome)
            .border_1()
            .border_color(p.border_hairline)
            .shadow_sm()
            .overflow_hidden()
            .child(rows);
        Some(list)
    }

    fn render_text_editor(
        &self,
        p: Palette,
        mono: SharedString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let mut pane = v_flex()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .bg(p.bg_canvas)
            .overflow_hidden();

        if self.open.as_ref().is_some_and(|open| open.external_change) {
            let reload_entity = cx.entity();
            let keep_entity = cx.entity();
            pane = pane.child(
                h_flex()
                    .h(px(36.))
                    .px(px(16.))
                    .gap(px(12.))
                    .items_center()
                    .justify_between()
                    .bg(p.warning_bg)
                    .border_b_1()
                    .border_color(p.warning_border)
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                Icon::new(IconName::Info)
                                    .size(px(14.))
                                    .text_color(p.warning),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(p.fg_primary)
                                    .child("This file changed on disk."),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .items_center()
                            .child(
                                Button::new("ext-reload-text")
                                    .outline()
                                    .xsmall()
                                    .label("Reload")
                                    .on_click(move |_, _, cx| {
                                        reload_entity
                                            .update(cx, |this, cx| this.reload_external(cx));
                                    }),
                            )
                            .child(
                                Button::new("ext-keep-text")
                                    .ghost()
                                    .xsmall()
                                    .label("Keep mine")
                                    .on_click(move |_, window, cx| {
                                        keep_entity.update(cx, |this, cx| this.cancel(window, cx));
                                    }),
                            ),
                    ),
            );
        }

        pane.child(
            div()
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .px(px(8.))
                .py(px(6.))
                .font_family(mono)
                .text_size(px(13.))
                .child(
                    CodeEditor::new(&self.text_editor)
                        .appearance(false)
                        .bordered(false)
                        .h_full(),
                ),
        )
    }

    fn short_project_path(&self) -> (String, String) {
        let Some(open) = self.open.as_ref() else {
            return (String::new(), String::new());
        };
        let full = open.path.display().to_string();
        let short = if let Some(project) = self.project_ix.and_then(|ix| self.projects.get(ix)) {
            if let Ok(rel) = open.path.strip_prefix(&project.root) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                if rel_str.is_empty() {
                    project.name.clone()
                } else {
                    format!("{}/{rel_str}", project.name)
                }
            } else {
                open.path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|f| format!("{}/{f}", project.name))
                    .unwrap_or_else(|| full.clone())
            }
        } else {
            open.path
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| full.clone())
        };
        (short, full)
    }

    fn render_save_status(&self, p: Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        match self.save_phase {
            SavePhase::Idle => None,
            SavePhase::Saving => Some(
                div()
                    .flex_shrink_0()
                    .text_size(px(11.5))
                    .line_height(px(16.))
                    .text_color(p.fg_tertiary)
                    .child("Saving…")
                    .into_any_element(),
            ),
            SavePhase::Saved => Some(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(4.))
                    .text_color(p.fg_secondary)
                    .child(Icon::new(IconName::Check).size(px(12.)))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .line_height(px(16.))
                            .child("Saved"),
                    )
                    .into_any_element(),
            ),
            SavePhase::Failed => {
                let entity = cx.entity();
                Some(
                    div()
                        .id("save-retry")
                        .flex_shrink_0()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .bg(p.warning_bg)
                        .border_1()
                        .border_color(p.warning_border)
                        .text_size(px(11.5))
                        .line_height(px(16.))
                        .text_color(p.warning)
                        .cursor_pointer()
                        .hover(|style| style.bg(p.warning_border))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            entity.update(cx, |this, cx| this.retry_save(cx));
                        })
                        .child("Couldn't save – Retry")
                        .into_any_element(),
                )
            }
        }
    }

    fn render_footer(
        &self,
        p: Palette,
        mono: SharedString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let (path_short, path_full) = self.short_project_path();
        let status = self.status.clone();
        let unreadable_count = self
            .open
            .as_ref()
            .map(|open| open.document.unreadable_lines().len())
            .unwrap_or(0);

        let mut left = h_flex().flex_1().min_w(px(0.)).items_center().gap(px(8.));
        if let Some(save) = self.render_save_status(p, cx) {
            left = left.child(save);
        }
        if !status.is_empty() {
            left = left.child(
                div()
                    .min_w(px(0.))
                    .text_size(px(11.5))
                    .line_height(px(16.))
                    .text_color(p.fg_secondary)
                    .truncate()
                    .child(status),
            );
        } else if !path_short.is_empty() {
            left = left.child(
                div()
                    .id("footer-path")
                    .min_w(px(0.))
                    .font_family(mono.clone())
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(p.fg_tertiary)
                    .truncate()
                    .tooltip(hover_tip(path_full))
                    .child(path_short),
            );
        }
        if unreadable_count > 0 && !self.diff_open {
            let entity = cx.entity();
            let note = if unreadable_count == 1 {
                "1 line Envy can't read".to_string()
            } else {
                format!("{unreadable_count} lines Envy can't read")
            };
            left = left.child(
                h_flex()
                    .id("footer-unreadable")
                    .h(px(20.))
                    .px(px(6.))
                    .flex_shrink_0()
                    .items_center()
                    .rounded(px(4.))
                    .bg(p.bg_kbd)
                    .border_1()
                    .border_color(p.border_hairline)
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(p.fg_secondary)
                    .cursor_pointer()
                    .hover(move |style| style.bg(p.bg_hover).text_color(p.fg_primary))
                    .tooltip(hover_tip("View lines Envy can't read"))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        entity.update(cx, |this, cx| {
                            this.overlay = Overlay::Unreadable;
                            cx.notify();
                        });
                    })
                    .child(note),
            );
        }

        let hints: &[(&str, &str)] = if self.diff_open {
            &[
                ("↑↓", "Move"),
                ("Space", "Reveal"),
                ("]", "Other file"),
                ("Esc", "Close"),
            ]
        } else if self.editor.is_some() {
            &[
                ("Tab", "Next cell"),
                ("Shift+Tab", "Previous cell"),
                ("Enter", "Done"),
                ("Esc", "Cancel"),
            ]
        } else if self.editor_view == EditorView::Text {
            &[("Ctrl+Z", "Undo"), ("Ctrl+E", "Table"), ("?", "Shortcuts")]
        } else {
            match self.zone {
                Zone::Projects => &[
                    ("↑↓", "Move"),
                    ("Enter", "Open"),
                    ("F", "Pin"),
                    ("Tab", "Next"),
                    ("?", "Shortcuts"),
                ],
                Zone::Tabs => &[
                    ("←→", "Move"),
                    ("Enter", "Open"),
                    ("Shift+Enter", "Compare"),
                    ("Tab", "Next"),
                ],
                Zone::Keys => &[
                    ("↑↓", "Move"),
                    ("Enter", "Edit"),
                    ("Ctrl+Space", "Toggle"),
                    ("Space", "Reveal"),
                    ("?", "Shortcuts"),
                ],
            }
        };

        let mut right = h_flex().flex_shrink_0().items_center().gap(px(12.));
        for (keys, label) in hints {
            right = right.child(footer_hint(keys, label, p, mono.clone()));
        }

        h_flex()
            .h(FOOTER_H)
            .flex_shrink_0()
            .px(px(12.))
            .gap(px(12.))
            .items_center()
            .justify_between()
            .bg(p.bg_chrome)
            .border_t_1()
            .border_color(p.border_hairline)
            .child(left)
            .child(right)
    }

    fn render_empty_state(
        &self,
        p: Palette,
        mono: SharedString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let entity = cx.entity();
        v_flex()
            .flex_1()
            .h_full()
            .items_center()
            .justify_center()
            .gap(px(16.))
            .bg(p.bg_canvas)
            .text_color(p.fg_primary)
            .child(
                div()
                    .size(px(48.))
                    .rounded(px(12.))
                    .bg(p.accent_subtle)
                    .border_1()
                    .border_color(p.accent_ring)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::FolderPlus)
                            .size(px(22.))
                            .text_color(p.accent),
                    ),
            )
            .child(
                v_flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.fg_primary)
                            .child("Add a folder"),
                    )
                    .child(
                        div()
                            .w(px(340.))
                            .text_size(px(13.))
                            .line_height(px(19.))
                            .text_center()
                            .text_color(p.fg_secondary)
                            .child(
                                "Choose a folder to list and edit its .env files, grouped by project.",
                            ),
                    ),
            )
            .child(
                v_flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Button::new("welcome-add")
                            .primary()
                            .icon(IconName::FolderPlus)
                            .label("Add folder")
                            .on_click(move |_, window, cx| {
                                entity.update(cx, |this, cx| this.open_add_folder(window, cx));
                            }),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .line_height(px(16.))
                                    .text_color(p.fg_tertiary)
                                    .child("Or press"),
                            )
                            .child(kbd_chip("Ctrl+O", p, mono)),
                    ),
            )
    }
}

impl EnvyApp {
    fn render_diff_view(
        &self,
        p: Palette,
        mono: SharedString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let (left_name, left_suffix) = self.side_name(DiffSide::Left);
        let (right_name, right_suffix) = self.side_name(DiffSide::Right);
        let left_label = labeled_name(&left_name, &left_suffix);
        let right_label = labeled_name(&right_name, &right_suffix);
        let one_file = self
            .project_ix
            .and_then(|ix| self.projects.get(ix))
            .is_some_and(|project| project.files.len() < 2);
        let keys_focused = self.zone == Zone::Keys;

        let pane = v_flex()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .overflow_hidden()
            .bg(p.bg_canvas);

        if one_file {
            return pane.child(
                div()
                    .p(px(24.))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(p.fg_tertiary)
                    .child("This project has one env file."),
            );
        }

        let Some(report) = self.diff_report.clone() else {
            return pane.child(
                h_flex()
                    .p(px(24.))
                    .gap(px(8.))
                    .items_center()
                    .text_size(px(13.))
                    .text_color(p.fg_tertiary)
                    .child(
                        Icon::new(IconName::LoaderCircle)
                            .size(px(14.))
                            .text_color(p.accent),
                    )
                    .child("Comparing"),
            );
        };

        let left_only_count = report
            .rows
            .iter()
            .filter(|r| r.kind == diff::DiffKind::OnlyLeft)
            .count();
        let right_only_count = report
            .rows
            .iter()
            .filter(|r| r.kind == diff::DiffKind::OnlyRight)
            .count();

        let missing_left_entity = cx.entity();
        let missing_right_entity = cx.entity();

        let header = h_flex()
            .h(DIFF_HEADER_H)
            .flex_shrink_0()
            .px(px(16.))
            .items_center()
            .bg(p.bg_chrome.opacity(0.65))
            .border_b_1()
            .border_color(p.border_hairline)
            .child(
                div()
                    .w(DIFF_KEY_COL_W)
                    .flex_shrink_0()
                    .pr(px(12.))
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.fg_tertiary)
                    .child("KEY"),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .pr(px(8.))
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .items_center()
                            .gap(px(6.))
                            .child(side_badge("L", p))
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .font_family(mono.clone())
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.fg_primary)
                                    .child(file_title(&left_name, &left_suffix, p)),
                            ),
                    )
                    .when(right_only_count > 0, |bar| {
                        bar.child(
                            div().flex_shrink_0().child(
                                Button::new("copy-missing-to-left")
                                    .outline()
                                    .xsmall()
                                    .icon(IconName::ArrowLeft)
                                    .label("Copy missing")
                                    .tooltip(format!(
                                        "Copy {right_only_count} missing into {left_label}"
                                    ))
                                    .on_click(move |_, _, cx| {
                                        missing_left_entity.update(cx, |this, cx| {
                                            this.copy_missing(DiffSide::Right, cx);
                                        });
                                    }),
                            ),
                        )
                    }),
            )
            .child(
                div()
                    .w(DIFF_GUTTER_W)
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::ArrowLeftRight)
                            .size(px(12.))
                            .text_color(p.fg_tertiary.opacity(0.65)),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .pl(px(8.))
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .items_center()
                            .gap(px(6.))
                            .child(side_badge("R", p))
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .font_family(mono.clone())
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.fg_primary)
                                    .child(file_title(&right_name, &right_suffix, p)),
                            ),
                    )
                    .when(left_only_count > 0, |bar| {
                        bar.child(
                            div().flex_shrink_0().child(
                                Button::new("copy-missing-to-right")
                                    .outline()
                                    .xsmall()
                                    .icon(IconName::ArrowRight)
                                    .label("Copy missing")
                                    .tooltip(format!(
                                        "Copy {left_only_count} missing into {right_label}"
                                    ))
                                    .on_click(move |_, _, cx| {
                                        missing_right_entity.update(cx, |this, cx| {
                                            this.copy_missing(DiffSide::Left, cx);
                                        });
                                    }),
                            ),
                        )
                    }),
            );

        let mut body = v_flex()
            .id("diff-body")
            .size_full()
            .py(px(4.))
            .gap(px(2.))
            .overflow_y_scroll()
            .track_scroll(&self.diff_scroll);

        if report.rows.is_empty() {
            body = body.child(
                div()
                    .px(px(16.))
                    .py(px(24.))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(p.fg_tertiary)
                    .child("Both files are empty."),
            );
        }

        for (index, row) in report.rows.iter().enumerate() {
            let (status_icon, glyph_color, tint) = match row.kind {
                diff::DiffKind::Same => (IconName::Equal, p.fg_tertiary, None),
                diff::DiffKind::Different => {
                    (IconName::EqualNot, p.warning, Some(p.diff_different_bg))
                }
                diff::DiffKind::OnlyLeft => (IconName::Plus, p.accent, Some(p.diff_only_left_bg)),
                diff::DiffKind::OnlyRight => {
                    (IconName::Minus, p.warning, Some(p.diff_only_right_bg))
                }
                diff::DiffKind::Disabled => (IconName::ToggleLeft, p.fg_tertiary, None),
            };
            let selected = keys_focused && self.diff_cursor == index;
            let shown = self.visible(&row.key);
            let left_empty = row.left.as_deref().is_some_and(str::is_empty);
            let right_empty = row.right.as_deref().is_some_and(str::is_empty);
            let left_text = row
                .left
                .as_deref()
                .map(|value| display_value(value, shown))
                .unwrap_or_else(|| "—".into());
            let right_text = row
                .right
                .as_deref()
                .map(|value| display_value(value, shown))
                .unwrap_or_else(|| "—".into());
            let left_color = if row.left.is_none() {
                p.fg_tertiary.opacity(0.6)
            } else if row.left_disabled {
                p.fg_tertiary
            } else if left_empty {
                p.fg_tertiary
            } else if shown {
                p.fg_primary
            } else {
                p.fg_secondary
            };
            let right_color = if row.right.is_none() {
                p.fg_tertiary.opacity(0.6)
            } else if row.right_disabled {
                p.fg_tertiary
            } else if right_empty {
                p.fg_tertiary
            } else if shown {
                p.fg_primary
            } else {
                p.fg_secondary
            };

            let click_entity = cx.entity();
            let to_right = cx.entity();
            let to_left = cx.entity();
            let diff_group = SharedString::from(format!("diff-row-group-{index}"));
            let can_copy_right = row.left.is_some() && row.kind != diff::DiffKind::Same;
            let can_copy_left = row.right.is_some() && row.kind != diff::DiffKind::Same;

            let mut line = h_flex()
                .id(SharedString::from(format!("diff-row-{index}")))
                .group(diff_group.clone())
                .mx(px(8.))
                .px(px(7.))
                .h(DIFF_ROW_H)
                .flex_shrink_0()
                .rounded(px(6.))
                .border_1()
                .border_color(if selected {
                    p.accent_ring
                } else {
                    p.bg_canvas.opacity(0.)
                })
                .items_center()
                .overflow_hidden()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    click_entity.update(cx, |this, cx| {
                        this.zone = Zone::Keys;
                        this.diff_cursor = index;
                        this.focus_self(window, cx);
                        cx.notify();
                    });
                });

            if selected && tint.is_none() {
                line = line.bg(p.bg_selected_accent);
            } else if let Some(bg) = tint {
                line = line.bg(bg);
            } else {
                line = line.hover(move |style| style.bg(p.bg_hover));
            }

            let key_col = h_flex()
                .id(SharedString::from(format!("diff-key-{index}")))
                .w(DIFF_KEY_COL_W)
                .flex_shrink_0()
                .pr(px(12.))
                .gap(px(8.))
                .items_center()
                .min_w(px(0.))
                .tooltip(hover_tip(row.key.clone()))
                .child(
                    div()
                        .w(px(14.))
                        .h(px(14.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(status_icon).size(px(12.)).text_color(glyph_color)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .font_family(mono.clone())
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .truncate()
                        .text_color(
                            if matches!(row.kind, diff::DiffKind::Same | diff::DiffKind::Disabled) {
                                p.fg_secondary
                            } else {
                                p.fg_primary
                            },
                        )
                        .child(row.key.clone()),
                );

            let mut left_col = h_flex()
                .flex_1()
                .min_w(px(0.))
                .pr(px(8.))
                .gap(px(6.))
                .items_center();
            if row.left_disabled {
                left_col = left_col.child(off_badge(p));
            }
            left_col = left_col.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .font_family(mono.clone())
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .truncate()
                    .text_color(left_color)
                    .child(left_text),
            );

            let mut gutter = h_flex()
                .w(DIFF_GUTTER_W)
                .flex_shrink_0()
                .gap(px(2.))
                .items_center()
                .justify_center();

            if can_copy_right {
                let tip = if row.left_disabled {
                    format!("Copy to {right_label} (off)")
                } else {
                    format!("Copy to {right_label}")
                };
                gutter = gutter.child(
                    row_icon_button(
                        SharedString::from(format!("diff-to-right-{index}")),
                        IconName::ArrowRight,
                        tip,
                    )
                    .text_color(p.fg_secondary)
                    .on_click(move |_, _, cx| {
                        to_right.update(cx, |this, cx| {
                            this.copy_diff_row(index, DiffSide::Left, cx);
                        });
                    }),
                );
            }
            if can_copy_left {
                let tip = if row.right_disabled {
                    format!("Copy to {left_label} (off)")
                } else {
                    format!("Copy to {left_label}")
                };
                gutter = gutter.child(
                    row_icon_button(
                        SharedString::from(format!("diff-to-left-{index}")),
                        IconName::ArrowLeft,
                        tip,
                    )
                    .text_color(p.fg_secondary)
                    .on_click(move |_, _, cx| {
                        to_left.update(cx, |this, cx| {
                            this.copy_diff_row(index, DiffSide::Right, cx);
                        });
                    }),
                );
            }

            let mut right_col = h_flex()
                .flex_1()
                .min_w(px(0.))
                .pl(px(8.))
                .gap(px(6.))
                .items_center();
            if row.right_disabled {
                right_col = right_col.child(off_badge(p));
            }
            right_col = right_col.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .font_family(mono.clone())
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .truncate()
                    .text_color(right_color)
                    .child(right_text),
            );

            line = line
                .child(key_col)
                .child(left_col)
                .child(gutter)
                .child(right_col);
            body = body.child(line);
        }

        pane.child(header).child(
            div()
                .id("diff-scroll-host")
                .relative()
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .overflow_hidden()
                .child(body)
                .child(Scrollbar::vertical(&self.diff_scroll).mode(ScrollbarMode::Hover)),
        )
    }

    fn render_new_file_sources(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (source, keys_only, source_focus) = match &self.file_prompt {
            FilePrompt::Create {
                source,
                keys_only,
                source_focus,
                ..
            } => (*source, *keys_only, *source_focus),
            _ => (0, true, false),
        };
        let files = self
            .project_ix
            .and_then(|ix| self.projects.get(ix))
            .map(|project| project.files.clone())
            .unwrap_or_default();
        let names: Vec<&str> = files.iter().map(|file| file.name.as_str()).collect();
        let order = copy_source_order(&names);
        let labels = self.current_tab_labels();

        let mut row = h_flex()
            .id("new-file-sources")
            .w_full()
            .px(px(12.))
            .py(px(6.))
            .gap(px(6.))
            .items_center()
            .flex_wrap()
            .bg(p.bg_canvas)
            .border_b_1()
            .border_color(p.border_hairline)
            .child(self.source_choice(0, "Start empty", "", source, source_focus, p, cx));

        if !order.is_empty() {
            row = row.child(
                div()
                    .pl(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(p.fg_tertiary)
                    .child("Copy from"),
            );
            for (slot, file_index) in order.iter().copied().enumerate() {
                let name = files[file_index].name.clone();
                let suffix = labels
                    .get(file_index)
                    .map(|label| label.suffix.clone())
                    .unwrap_or_default();
                row = row.child(self.source_choice(
                    slot + 1,
                    &name,
                    &suffix,
                    source,
                    source_focus,
                    p,
                    cx,
                ));
            }
        }

        let show_toggle = source > 0
            && order
                .get(source - 1)
                .and_then(|&index| files.get(index))
                .is_some_and(|file| !is_example_filename(&file.name));
        if show_toggle {
            row = row.child(self.copy_mode_toggle(keys_only, p, cx));
        }
        row
    }

    fn source_choice(
        &self,
        index: usize,
        label: &str,
        suffix: &str,
        selected: usize,
        focused: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let chosen = index == selected;
        let entity = cx.entity();
        let mut chip = h_flex()
            .id(SharedString::from(format!("new-file-source-{index}")))
            .px(px(8.))
            .h(px(22.))
            .rounded(px(4.))
            .items_center()
            .cursor_pointer()
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(if chosen { p.accent } else { p.fg_secondary })
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                entity.update(cx, |this, cx| {
                    if let FilePrompt::Create {
                        source,
                        source_focus,
                        ..
                    } = &mut this.file_prompt
                    {
                        *source = index;
                        *source_focus = false;
                    }
                    this.commit_file_prompt(window, cx);
                });
            });
        if chosen {
            chip = chip.bg(p.accent_subtle);
            if focused {
                chip = chip.border_1().border_color(p.accent_ring);
            }
        } else {
            chip = chip.hover(move |style| style.bg(p.bg_hover).text_color(p.fg_primary));
        }
        chip.child(file_title(label, suffix, p))
    }

    fn copy_mode_toggle(
        &self,
        keys_only: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        h_flex()
            .ml(px(6.))
            .gap(px(2.))
            .p(px(2.))
            .rounded(px(6.))
            .bg(p.bg_chrome)
            .child(self.copy_mode_button(true, "Keys only", keys_only, p, cx))
            .child(self.copy_mode_button(false, "Keys and values", keys_only, p, cx))
    }

    fn copy_mode_button(
        &self,
        want: bool,
        label: &'static str,
        keys_only: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = keys_only == want;
        let entity = cx.entity();
        let mut button = h_flex()
            .id(SharedString::from(if want {
                "copy-keys-only"
            } else {
                "copy-keys-and-values"
            }))
            .px(px(8.))
            .h(px(20.))
            .rounded(px(4.))
            .items_center()
            .cursor_pointer()
            .text_size(px(11.5))
            .line_height(px(16.))
            .text_color(if selected { p.accent } else { p.fg_tertiary })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                entity.update(cx, |this, cx| {
                    if let FilePrompt::Create { keys_only, .. } = &mut this.file_prompt {
                        *keys_only = want;
                    }
                    cx.notify();
                });
            });
        if selected {
            button = button.bg(p.bg_canvas);
        }
        button.child(label)
    }

    fn delete_file_panel(&self, name: String, p: Palette, cx: &mut Context<Self>) -> gpui_kit::Div {
        let entity = cx.entity();
        let cancel = entity.clone();
        div().child(
            v_flex()
                .id("delete-file-dialog")
                .w(px(320.))
                .bg(p.bg_surface)
                .border_1()
                .border_color(p.border_control)
                .rounded(px(10.))
                .shadow_lg()
                .overflow_hidden()
                .child(
                    div()
                        .px(px(16.))
                        .pt(px(16.))
                        .pb(px(12.))
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.fg_primary)
                        .child(format!("Delete {name}?")),
                )
                .child(
                    h_flex()
                        .px(px(16.))
                        .pb(px(16.))
                        .gap(px(8.))
                        .justify_end()
                        .child(
                            h_flex()
                                .id("delete-file-cancel")
                                .px(px(12.))
                                .h(px(28.))
                                .rounded(px(6.))
                                .items_center()
                                .cursor_pointer()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(p.fg_secondary)
                                .hover(move |style| style.bg(p.bg_hover))
                                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                    cx.stop_propagation();
                                    cancel.update(cx, |this, cx| this.close_overlay(window, cx));
                                })
                                .child("Cancel"),
                        )
                        .child(
                            h_flex()
                                .id("delete-file-confirm")
                                .px(px(12.))
                                .h(px(28.))
                                .rounded(px(6.))
                                .items_center()
                                .cursor_pointer()
                                .bg(p.danger_bg)
                                .border_1()
                                .border_color(p.danger_border)
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.danger)
                                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                    cx.stop_propagation();
                                    entity.update(cx, |this, cx| {
                                        let Overlay::DeleteFile { file_ix, .. } = &this.overlay
                                        else {
                                            return;
                                        };
                                        let file_ix = *file_ix;
                                        this.delete_tab(file_ix, window, cx);
                                    });
                                })
                                .child("Delete"),
                        ),
                ),
        )
    }

    fn tab_menu_layer(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(menu) = &self.tab_menu else {
            return div();
        };
        let file_ix = menu.file_ix;
        let cursor = menu.cursor;
        let entity = cx.entity();
        let close = entity.clone();
        let nudge = (file_ix as f32 * 72.).min(420.);
        let items = ["Rename", "Duplicate", "Reveal in Explorer", "Delete file"];
        let mut panel = v_flex()
            .id("tab-menu")
            .w(px(220.))
            .py(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(p.border_control)
            .bg(p.bg_surface)
            .shadow_lg()
            .occlude();
        for (index, label) in items.iter().enumerate() {
            if index == 3 {
                panel = panel.child(div().mx(px(8.)).my(px(4.)).h(px(1.)).bg(p.border_hairline));
            }
            let chosen = cursor == index;
            let danger = index == 3;
            let item_entity = entity.clone();
            let mut item = h_flex()
                .id(SharedString::from(format!("tab-menu-{index}")))
                .h(px(28.))
                .mx(px(4.))
                .px(px(8.))
                .rounded(px(5.))
                .items_center()
                .cursor_pointer()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(if danger { p.danger } else { p.fg_primary })
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    item_entity.update(cx, |this, cx| {
                        if let Some(menu) = &mut this.tab_menu {
                            menu.cursor = index;
                        }
                        this.activate_tab_menu(window, cx);
                    });
                });
            if chosen {
                item = item.bg(if danger { p.danger_bg } else { p.accent_subtle });
            } else {
                let hover_bg = if danger { p.danger_bg } else { p.bg_hover };
                item = item.hover(move |style| style.bg(hover_bg));
            }
            panel = panel.child(item.child(*label));
        }

        div()
            .absolute()
            .inset_0()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                close.update(cx, |this, cx| this.close_tab_menu(window, cx));
            })
            .on_mouse_down(MouseButton::Right, {
                let close = entity;
                move |_, window, cx| {
                    close.update(cx, |this, cx| this.close_tab_menu(window, cx));
                }
            })
            .child(
                div()
                    .absolute()
                    .top(TAB_BAR_H)
                    .left(px(self.sidebar_width + 12. + nudge))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .child(panel),
            )
    }

    fn overlay_view(
        &self,
        p: Palette,
        mono: SharedString,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let entity = cx.entity();
        let panel = match &self.overlay {
            Overlay::Settings => self.settings_panel(p, mono, cx),
            Overlay::AddFolder => self.folder_panel(p, cx),
            Overlay::Paste(plan) => self.paste_panel(plan, p, mono, cx),
            Overlay::Shortcuts => self.shortcuts_panel(p, mono, cx),
            Overlay::Unreadable => self.unreadable_panel(p, mono, cx),
            Overlay::DeleteFile { name, .. } => self.delete_file_panel(name.clone(), p, cx),
            Overlay::None => div(),
        };
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.))
            .bg(p.backdrop)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                entity.update(cx, |this, cx| this.close_overlay(window, cx));
            })
            .child(
                div()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(panel),
            )
    }

    fn settings_panel(
        &self,
        p: Palette,
        mono: SharedString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let entity = cx.entity();
        let theme = self.config.settings.theme;
        let clear = self.config.settings.clear_clipboard;

        let mut folders = v_flex()
            .rounded(px(6.))
            .border_1()
            .border_color(p.border_hairline)
            .bg(p.bg_chrome)
            .overflow_hidden();

        if self.config.folders.is_empty() {
            folders = folders.child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(p.fg_tertiary)
                    .child("No folders added."),
            );
        } else {
            for (index, folder) in self.config.folders.iter().enumerate() {
                let rm_entity = entity.clone();
                let mut row = h_flex()
                    .px(px(10.))
                    .py(px(6.))
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .font_family(mono.clone())
                            .text_size(px(11.5))
                            .line_height(px(16.))
                            .text_color(p.fg_primary)
                            .truncate()
                            .child(folder.display().to_string()),
                    )
                    .child(
                        row_icon_button(
                            SharedString::from(format!("rm-folder-{index}")),
                            IconName::Trash,
                            "Remove folder",
                        )
                        .text_color(p.fg_secondary)
                        .on_click(move |_, _, cx| {
                            rm_entity.update(cx, |this, cx| this.remove_folder(index, cx));
                        }),
                    );
                if index > 0 {
                    row = row.border_t_1().border_color(p.border_hairline);
                }
                folders = folders.child(row);
            }
        }

        let header = overlay_header("Settings", p, entity.clone());

        v_flex()
            .w(px(480.))
            .bg(p.bg_surface)
            .border_1()
            .border_color(p.border_control)
            .rounded(px(10.))
            .shadow_lg()
            .overflow_hidden()
            .child(header)
            .child(
                v_flex()
                    .p(px(16.))
                    .gap(px(16.))
                    .child(
                        v_flex().gap(px(6.)).child(setting_label("Theme", p)).child(
                            h_flex()
                                .gap(px(6.))
                                .child(theme_option(
                                    "theme-system",
                                    IconName::Monitor,
                                    "System",
                                    theme == ThemeSetting::System,
                                    ThemeSetting::System,
                                    p,
                                    cx,
                                ))
                                .child(theme_option(
                                    "theme-light",
                                    IconName::Sun,
                                    "Light",
                                    theme == ThemeSetting::Light,
                                    ThemeSetting::Light,
                                    p,
                                    cx,
                                ))
                                .child(theme_option(
                                    "theme-dark",
                                    IconName::Moon,
                                    "Dark",
                                    theme == ThemeSetting::Dark,
                                    ThemeSetting::Dark,
                                    p,
                                    cx,
                                )),
                        ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                v_flex()
                                    .gap(px(2.))
                                    .child(setting_label("Clear clipboard", p))
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .line_height(px(15.))
                                            .text_color(p.fg_tertiary)
                                            .child("Wipe copied values after 30 seconds."),
                                    ),
                            )
                            .child({
                                let mut btn = Button::new("clear-clip")
                                    .outline()
                                    .small()
                                    .icon(if clear {
                                        IconName::ClipboardCheck
                                    } else {
                                        IconName::Clipboard
                                    })
                                    .label(if clear { "On (30s)" } else { "Off" })
                                    .selected(clear)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.toggle_clear_clipboard(cx);
                                            });
                                        }
                                    });
                                if clear {
                                    btn = btn.text_color(p.accent);
                                }
                                btn
                            }),
                    )
                    .child(
                        v_flex()
                            .gap(px(6.))
                            .child(setting_label("Global hotkey", p))
                            .child(lined_field(&self.hotkey_input, p)),
                    )
                    .child(
                        v_flex()
                            .gap(px(6.))
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .child(setting_label("Folders", p))
                                    .child(
                                        Button::new("settings-add-folder")
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::FolderPlus)
                                            .label("Add folder")
                                            .on_click({
                                                let entity = entity.clone();
                                                move |_, window, cx| {
                                                    entity.update(cx, |this, cx| {
                                                        this.open_add_folder(window, cx);
                                                    });
                                                }
                                            }),
                                    ),
                            )
                            .child(folders),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .line_height(px(15.))
                            .text_color(p.fg_tertiary)
                            .child(
                                "Values stay in your .env files and are never saved in settings.",
                            ),
                    ),
            )
    }

    fn folder_panel(&self, p: Palette, cx: &mut Context<Self>) -> gpui_kit::Div {
        let entity = cx.entity();
        let browse_entity = entity.clone();
        v_flex()
            .w(px(460.))
            .bg(p.bg_surface)
            .border_1()
            .border_color(p.border_control)
            .rounded(px(10.))
            .shadow_lg()
            .overflow_hidden()
            .child(overlay_header("Add folder", p, entity))
            .child(
                v_flex()
                    .p(px(16.))
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .line_height(px(18.))
                            .text_color(p.fg_secondary)
                            .child("Enter a directory path to list its .env files."),
                    )
                    .child(lined_field(&self.folder_input, p))
                    .child(
                        h_flex().justify_end().gap(px(8.)).child(
                            Button::new("browse-folder")
                                .outline()
                                .small()
                                .icon(IconName::Folder)
                                .label("Browse")
                                .on_click(move |_, window, cx| {
                                    browse_entity
                                        .update(cx, |this, cx| this.open_add_folder(window, cx));
                                }),
                        ),
                    ),
            )
    }

    fn paste_panel(
        &self,
        plan: &PastePlan,
        p: Palette,
        mono: SharedString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let entity = cx.entity();
        let apply_entity = entity.clone();
        let cancel_entity = entity.clone();

        let mut list = v_flex()
            .id("paste-list")
            .max_h(px(280.))
            .overflow_y_scroll()
            .gap(px(2.));
        for change in &plan.changes {
            let (badge, badge_color, badge_bg, key, value) = match change {
                PasteChange::Add { key, value } => ("add", p.accent, p.accent_subtle, key, value),
                PasteChange::Overwrite { key, value, .. } => {
                    ("update", p.warning, p.warning_bg, key, value)
                }
            };
            list = list.child(
                h_flex()
                    .h(px(30.))
                    .flex_shrink_0()
                    .px(px(8.))
                    .gap(px(10.))
                    .rounded(px(6.))
                    .items_center()
                    .bg(p.bg_chrome)
                    .child(
                        div()
                            .w(px(52.))
                            .flex_shrink_0()
                            .px(px(6.))
                            .py(px(1.))
                            .rounded(px(4.))
                            .bg(badge_bg)
                            .text_center()
                            .text_size(px(10.5))
                            .line_height(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(badge_color)
                            .child(badge),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .font_family(mono.clone())
                            .text_size(px(12.5))
                            .line_height(px(17.))
                            .text_color(p.fg_primary)
                            .truncate()
                            .child(key.clone()),
                    )
                    .child(
                        div()
                            .max_w(px(160.))
                            .font_family(mono.clone())
                            .text_size(px(12.5))
                            .line_height(px(17.))
                            .text_color(p.fg_secondary)
                            .truncate()
                            .child(display_value(value, self.visible(key))),
                    ),
            );
        }

        v_flex()
            .w(px(520.))
            .bg(p.bg_surface)
            .border_1()
            .border_color(p.border_control)
            .rounded(px(10.))
            .shadow_lg()
            .overflow_hidden()
            .child(overlay_header(
                format!("Paste {} keys", plan.changes.len()),
                p,
                entity,
            ))
            .child(
                v_flex()
                    .p(px(16.))
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .line_height(px(18.))
                            .text_color(p.fg_secondary)
                            .child("Existing keys not in this paste stay unchanged."),
                    )
                    .child(list)
                    .child(
                        h_flex()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                Button::new("paste-cancel")
                                    .ghost()
                                    .small()
                                    .label("Cancel")
                                    .on_click(move |_, window, cx| {
                                        cancel_entity
                                            .update(cx, |this, cx| this.close_overlay(window, cx));
                                    }),
                            )
                            .child(
                                Button::new("paste-apply")
                                    .primary()
                                    .small()
                                    .label("Apply paste")
                                    .on_click(move |_, window, cx| {
                                        apply_entity.update(cx, |this, cx| {
                                            this.apply_overlay_paste(window, cx);
                                        });
                                    }),
                            ),
                    ),
            )
    }

    fn copy_missing(&mut self, from: DiffSide, cx: &mut Context<Self>) {
        let Some(report) = self.diff_report.clone() else {
            self.status = "Nothing to compare.".into();
            cx.notify();
            return;
        };
        let entries: Vec<(String, String, bool)> = report
            .rows
            .into_iter()
            .filter_map(|row| {
                let (kind, value, disabled) = match from {
                    DiffSide::Left => (diff::DiffKind::OnlyLeft, row.left, row.left_disabled),
                    DiffSide::Right => (diff::DiffKind::OnlyRight, row.right, row.right_disabled),
                };
                if row.kind == kind {
                    Some((row.key, value.unwrap_or_default(), disabled))
                } else {
                    None
                }
            })
            .collect();
        let to = match from {
            DiffSide::Left => DiffSide::Right,
            DiffSide::Right => DiffSide::Left,
        };
        self.put_entries(to, entries, cx);
    }

    fn copy_diff_row(&mut self, index: usize, from: DiffSide, cx: &mut Context<Self>) {
        let Some(row) = self
            .diff_report
            .as_ref()
            .and_then(|report| report.rows.get(index))
            .cloned()
        else {
            return;
        };
        let (value, disabled) = match from {
            DiffSide::Left => (row.left, row.left_disabled),
            DiffSide::Right => (row.right, row.right_disabled),
        };
        let Some(value) = value else {
            self.status = "That side has no value for this key.".into();
            cx.notify();
            return;
        };
        let to = match from {
            DiffSide::Left => DiffSide::Right,
            DiffSide::Right => DiffSide::Left,
        };
        self.put_entries(to, vec![(row.key, value, disabled)], cx);
    }

    fn put_entries(
        &mut self,
        side: DiffSide,
        entries: Vec<(String, String, bool)>,
        cx: &mut Context<Self>,
    ) {
        if entries.is_empty() {
            self.status = "Nothing to copy.".into();
            cx.notify();
            return;
        }
        let count = entries.len();
        let Some(path) = self.side_path(side) else {
            return;
        };
        let onto_open = self.open.as_ref().is_some_and(|open| open.path == path);
        if onto_open {
            self.commit_cell_if_open(cx);
            let Some(open) = self.open.as_mut() else {
                return;
            };
            open.mutate(|document| write_entries(document, &entries));
            let cloned = self
                .open
                .as_ref()
                .map(|open| open.document.clone())
                .unwrap_or_default();
            match side {
                DiffSide::Left => self.diff_left_doc = Some(cloned),
                DiffSide::Right => self.diff_right_doc = Some(cloned),
            }
            self.mark_edited(cx);
            self.status = format!("Copied {count} into the open file.");
        } else {
            let doc = match side {
                DiffSide::Left => self.diff_left_doc.as_mut(),
                DiffSide::Right => self.diff_right_doc.as_mut(),
            };
            let Some(doc) = doc else {
                self.status = "That file is still loading.".into();
                cx.notify();
                return;
            };
            write_entries(doc, &entries);
            let text = doc.serialize();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("file");
            self.status = format!("Copied {count} into {name}.");
            self.save_other(path, text, cx);
        }
        self.recompute_diff(cx);
        cx.notify();
    }

    fn save_other(&mut self, path: PathBuf, text: String, cx: &mut Context<Self>) {
        self.queue_save_text(path, text, cx);
    }

    fn unreadable_panel(
        &self,
        p: Palette,
        mono: SharedString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let entity = cx.entity();
        let close_entity = entity.clone();
        let lines: Vec<UnreadableLine> = self
            .open
            .as_ref()
            .map(|open| open.document.unreadable_lines())
            .unwrap_or_default();

        let mut list = v_flex()
            .id("unreadable-list")
            .rounded(px(6.))
            .border_1()
            .border_color(p.border_hairline)
            .bg(p.bg_chrome)
            .max_h(px(260.))
            .overflow_y_scroll();

        if lines.is_empty() {
            list = list.child(
                div()
                    .p(px(12.))
                    .text_size(px(12.5))
                    .line_height(px(17.))
                    .text_color(p.fg_tertiary)
                    .child("All lines in this file are readable."),
            );
        } else {
            for (ix, item) in lines.iter().enumerate() {
                let mut row = h_flex()
                    .id(SharedString::from(format!(
                        "unreadable-line-{}",
                        item.line_number
                    )))
                    .min_h(px(28.))
                    .px(px(10.))
                    .py(px(5.))
                    .gap(px(12.))
                    .items_center()
                    .tooltip(hover_tip(item.text.clone()));
                if ix > 0 {
                    row = row.border_t_1().border_color(p.border_hairline);
                }
                row = row
                    .child(
                        div()
                            .w(px(36.))
                            .flex_shrink_0()
                            .text_right()
                            .font_family(mono.clone())
                            .text_size(px(11.5))
                            .line_height(px(16.))
                            .text_color(p.fg_tertiary)
                            .child(item.line_number.to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .font_family(mono.clone())
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(p.fg_primary)
                            .truncate()
                            .child(item.text.clone()),
                    );
                list = list.child(row);
            }
        }

        v_flex()
            .w(px(540.))
            .bg(p.bg_surface)
            .border_1()
            .border_color(p.border_control)
            .rounded(px(10.))
            .shadow_lg()
            .overflow_hidden()
            .child(overlay_header("Lines Envy can't read", p, entity))
            .child(
                v_flex()
                    .p(px(16.))
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .line_height(px(17.))
                            .text_color(p.fg_secondary)
                            .child("These lines are left untouched when you save."),
                    )
                    .child(list),
            )
            .child(
                h_flex()
                    .px(px(16.))
                    .py(px(10.))
                    .bg(p.bg_chrome)
                    .border_t_1()
                    .border_color(p.border_hairline)
                    .items_center()
                    .justify_between()
                    .child(footer_hint("Esc", "Close", p, mono))
                    .child(
                        Button::new("unreadable-close")
                            .ghost()
                            .small()
                            .label("Close")
                            .on_click(move |_, window, cx| {
                                close_entity.update(cx, |this, cx| this.close_overlay(window, cx));
                            }),
                    ),
            )
    }

    fn shortcuts_panel(
        &self,
        p: Palette,
        mono: SharedString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let entity = cx.entity();
        let mid = SHORTCUTS.len().div_ceil(2);
        let mut columns = h_flex().gap(px(20.)).items_start();
        for range in [0..mid, mid..SHORTCUTS.len()] {
            let mut column = v_flex().flex_1().gap(px(4.));
            for (keys, what) in &SHORTCUTS[range] {
                column = column.child(
                    h_flex()
                        .h(px(26.))
                        .justify_between()
                        .gap(px(12.))
                        .px(px(6.))
                        .rounded(px(6.))
                        .items_center()
                        .child(
                            div()
                                .text_size(px(12.5))
                                .line_height(px(17.))
                                .text_color(p.fg_primary)
                                .child(*what),
                        )
                        .child(kbd_chips(keys, p, mono.clone())),
                );
            }
            columns = columns.child(column);
        }
        v_flex()
            .w(px(640.))
            .bg(p.bg_surface)
            .border_1()
            .border_color(p.border_control)
            .rounded(px(10.))
            .shadow_lg()
            .overflow_hidden()
            .child(overlay_header("Keyboard shortcuts", p, entity))
            .child(v_flex().p(px(16.)).gap(px(12.)).child(columns))
            .child(
                h_flex()
                    .px(px(16.))
                    .py(px(10.))
                    .bg(p.bg_chrome)
                    .border_t_1()
                    .border_color(p.border_hairline)
                    .items_center()
                    .gap(px(6.))
                    .child(kbd_chip("Esc", p, mono))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .line_height(px(15.))
                            .text_color(p.fg_tertiary)
                            .child("Close"),
                    ),
            )
    }
}

const SHORTCUTS: &[(&str, &str)] = &[
    ("↑  ↓", "Move. Down leaves a search"),
    ("Ctrl+Shift+N", "New file"),
    ("Right-click", "File menu"),
    ("Enter", "Close the cell, or open the project"),
    ("Tab", "Key, value, then the next key"),
    ("Shift+Tab", "Previous cell, or the previous area"),
    ("←", "Back to the project list"),
    ("←  →", "Move across file tabs"),
    ("Ctrl+Tab", "Next file"),
    ("Ctrl+1…9", "Open that file"),
    ("/  Ctrl+F", "Search this list"),
    ("Esc", "Clear the search, then leave it"),
    ("Shift+Enter", "Compare the focused tab"),
    ("Ctrl+click", "Compare a second tab"),
    ("d", "Compare with the next file"),
    ("]", "Change the second file"),
    ("f", "Pin the project"),
    ("c", "Copy the value"),
    ("Space", "Show the value"),
    ("a", "Show every value"),
    ("Ctrl+Space", "Turn key on or off"),
    ("Ctrl+E", "Switch table / plain text"),
    ("n", "New key"),
    ("Ctrl+Z", "Undo"),
    ("Ctrl+O", "Add a folder"),
    ("?", "This list"),
];

enum IoDone {
    Saved {
        seq: u64,
        path: PathBuf,
        text: String,
        result: std::io::Result<Option<std::time::SystemTime>>,
    },
    Loaded {
        epoch: u64,
        path: PathBuf,
        result: std::io::Result<OpenFile>,
    },
    Reloaded {
        epoch: u64,
        result: std::io::Result<OpenFile>,
    },
    Deleted {
        name: String,
        result: std::io::Result<()>,
    },
    Quit,
}

fn run_io(job: IoJob) -> IoDone {
    match job {
        IoJob::Save { seq, path, text } => {
            let result = save_text(&path, &text);
            IoDone::Saved {
                seq,
                path,
                text,
                result,
            }
        }
        IoJob::Load { epoch, path } => {
            let result = OpenFile::load(&path);
            IoDone::Loaded {
                epoch,
                path,
                result,
            }
        }
        IoJob::Reload { epoch, path } => {
            let result = OpenFile::load(&path);
            IoDone::Reloaded { epoch, result }
        }
        IoJob::Delete { path, name } => {
            let result = std::fs::remove_file(&path);
            IoDone::Deleted { name, result }
        }
        IoJob::Quit => IoDone::Quit,
    }
}

fn open_containing_folder(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map(|_| ())
    }
}

impl EnvyApp {
    fn focus_self_cx(&self, cx: &mut App) {
        let focus = self.focus.clone();
        self.window
            .update(cx, |_, window, cx| {
                window.focus(&focus, cx);
            })
            .ok();
    }

    fn open_pending_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.pending_file.clone() else {
            return;
        };
        let Some(project_ix) = self.project_ix else {
            return;
        };
        let Some(file_ix) = self
            .projects
            .get(project_ix)
            .and_then(|project| project.files.iter().position(|file| file.path == path))
        else {
            return;
        };
        self.pending_file = None;
        self.file_ix = Some(file_ix);
        self.load_selected_file(cx);
    }

    fn follow_pending_launch(&mut self, cx: &mut Context<Self>) {
        let Some(raw) = self.pending_launch.take() else {
            return;
        };
        if !raw.starts_with("envy:") {
            return;
        }
        match Deeplink::parse(&raw) {
            Ok(link) => self.follow_link_cx(link, cx),
            Err(err) => self.status = err.to_string(),
        }
    }

    fn persist_config(&mut self, cx: &mut Context<Self>) {
        self.config_dirty = true;
        if self.config_saving {
            return;
        }
        self.config_saving = true;
        let config = self.config.clone();
        self.config_dirty = false;
        cx.spawn(async move |this, async_cx| {
            let result = async_cx
                .background_executor()
                .spawn(async move { config.save() })
                .await;
            let _ = this.update(async_cx, |view, cx| {
                view.config_saving = false;
                if let Err(err) = result {
                    view.status = format!("Could not save settings: {err}");
                    cx.notify();
                }
                if view.config_dirty {
                    view.persist_config(cx);
                }
            });
        })
        .detach();
    }

    fn install_hotkey(&mut self, spec: String, announce: bool, cx: &mut Context<Self>) {
        cx.spawn(async move |this, async_cx| {
            let spec_for_result = spec.clone();
            let result = async_cx
                .background_executor()
                .spawn(async move {
                    hotkey_os::request(spec)
                        .recv()
                        .unwrap_or_else(|_| Err("Couldn't change the hotkey.".into()))
                })
                .await;
            let _ = this.update(async_cx, |view, cx| match result {
                Ok(()) => {
                    view.config.settings.global_hotkey = spec_for_result.clone();
                    if announce {
                        view.persist_config(cx);
                        view.status = format!("Hotkey is {spec_for_result}.");
                        cx.notify();
                    }
                }
                Err(err) => {
                    view.status = if announce {
                        format!("Hotkey not changed: {err}")
                    } else {
                        format!("Global hotkey off: {err}")
                    };
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn log_startup(&mut self, files: Option<Duration>, git: Option<Duration>) {
        if self.startup_logged {
            return;
        }
        self.startup_logged = true;
        let line = format!(
            "ui_ready_ms={} config_ms={} files_ms={} git_ms={}",
            self.ui_ready.as_millis(),
            self.config_load.as_millis(),
            files.unwrap_or_default().as_millis(),
            git.unwrap_or_default().as_millis(),
        );
        std::thread::Builder::new()
            .name("envy-startup-log".into())
            .spawn(move || crate::note(&line))
            .ok();
    }
}

fn apply_theme_setting(setting: ThemeSetting, window: &mut Window, cx: &mut App) {
    match setting {
        ThemeSetting::System => Theme::sync_system_appearance(Some(window), cx),
        ThemeSetting::Light => Theme::change(ThemeMode::Light, Some(window), cx),
        ThemeSetting::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
    }
    theme::apply_palette(cx);
}

fn theme_option(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    selected: bool,
    setting: ThemeSetting,
    p: Palette,
    cx: &mut Context<EnvyApp>,
) -> Button {
    let entity = cx.entity();
    let mut btn = Button::new(id)
        .outline()
        .small()
        .icon(icon)
        .label(label)
        .selected(selected)
        .on_click(move |_, window, cx| {
            entity.update(cx, |this, cx| this.set_theme(setting, window, cx));
        });
    if selected {
        btn = btn.text_color(p.accent);
    }
    btn
}

fn overlay_header(
    title: impl Into<SharedString>,
    p: Palette,
    entity: Entity<EnvyApp>,
) -> gpui_kit::Div {
    h_flex()
        .h(px(44.))
        .px(px(16.))
        .items_center()
        .justify_between()
        .bg(p.bg_chrome)
        .border_b_1()
        .border_color(p.border_hairline)
        .child(
            div()
                .text_size(px(14.))
                .line_height(px(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.fg_primary)
                .child(title.into()),
        )
        .child(
            row_icon_button("overlay-close", IconName::X, "Close · Esc")
                .text_color(p.fg_secondary)
                .on_click(move |_, window, cx| {
                    entity.update(cx, |this, cx| this.close_overlay(window, cx));
                }),
        )
}

fn setting_label(text: &'static str, p: Palette) -> gpui_kit::Div {
    div()
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.fg_primary)
        .child(text)
}

fn side_badge(tag: &'static str, p: Palette) -> gpui_kit::Div {
    div()
        .px(px(4.))
        .py(px(1.))
        .rounded(px(4.))
        .bg(p.accent_subtle)
        .text_size(px(10.))
        .line_height(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.accent)
        .child(tag)
}

fn off_badge(p: Palette) -> gpui_kit::Div {
    div()
        .px(px(5.))
        .py(px(1.))
        .flex_shrink_0()
        .rounded(px(4.))
        .bg(p.bg_kbd)
        .border_1()
        .border_color(p.border_hairline)
        .text_size(px(10.))
        .line_height(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.fg_tertiary)
        .child("off")
}

fn neutral_summary_chip(
    id: &'static str,
    icon: IconName,
    icon_color: gpui_kit::Hsla,
    text: String,
    p: Palette,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    toolbar_chip(
        id,
        icon,
        icon_color,
        text,
        p.bg_kbd,
        p.border_hairline,
        p.fg_secondary,
    )
}

/// A toolbar notice that keeps its own box. The label ellipsizes, and the
/// tooltip is the full sentence. Icon buttons beside it do not shrink.
fn toolbar_chip(
    id: &'static str,
    icon: IconName,
    icon_color: gpui_kit::Hsla,
    label: String,
    bg: gpui_kit::Hsla,
    border: gpui_kit::Hsla,
    text: gpui_kit::Hsla,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    h_flex()
        .id(id)
        .h(px(24.))
        .min_w(px(TOOLBAR_NOTICE_MIN_PX))
        .flex_shrink(TOOLBAR_NOTICE_SHRINK)
        .overflow_hidden()
        .px(px(8.))
        .gap(px(5.))
        .items_center()
        .rounded(px(999.))
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(text)
        .tooltip(hover_tip(label.clone()))
        .child(
            div()
                .flex_shrink_0()
                .child(Icon::new(icon).size(px(12.)).text_color(icon_color)),
        )
        .child(div().min_w(px(0.)).truncate().child(label))
}

fn kbd_chip(keys: &str, p: Palette, mono: SharedString) -> gpui_kit::Div {
    div()
        .px(px(5.))
        .py(px(1.))
        .rounded(px(4.))
        .bg(p.bg_kbd)
        .border_1()
        .border_color(p.border_control)
        .font_family(mono)
        .text_size(px(10.5))
        .line_height(px(14.))
        .text_color(p.fg_secondary)
        .whitespace_nowrap()
        .child(keys.to_string())
}

fn kbd_chips(keys: &str, p: Palette, mono: SharedString) -> gpui_kit::Div {
    let mut row = h_flex().items_center().gap(px(4.)).flex_shrink_0();
    for part in keys.split("  ") {
        row = row.child(kbd_chip(part, p, mono.clone()));
    }
    row
}

fn footer_hint(keys: &str, label: &str, p: Palette, mono: SharedString) -> gpui_kit::Div {
    h_flex()
        .items_center()
        .gap(px(5.))
        .child(kbd_chip(keys, p, mono))
        .child(
            div()
                .text_size(px(11.))
                .line_height(px(14.))
                .text_color(p.fg_tertiary)
                .whitespace_nowrap()
                .child(label.to_string()),
        )
}

fn v_divider(p: Palette) -> gpui_kit::Div {
    div()
        .w(px(1.))
        .h(px(16.))
        .mx(px(4.))
        .bg(p.border_hairline)
        .flex_shrink_0()
}

fn displayed_project_path(native: &str) -> String {
    match host_path_style() {
        PathStyle::Windows => native.to_string(),
        PathStyle::Unix => abbreviate_home(native, unix_home().as_deref()),
    }
}

fn unix_home() -> Option<String> {
    #[cfg(windows)]
    {
        None
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOME").ok().filter(|home| !home.is_empty())
    }
}

fn sidebar_section(label: &'static str, divider: bool, p: Palette) -> gpui_kit::Div {
    v_flex()
        .mx(px(8.))
        .px(px(8.))
        .pt(if divider { px(10.) } else { px(6.) })
        .pb(px(2.))
        .when(divider, |section| {
            section
                .mt(px(4.))
                .border_t_1()
                .border_color(p.border_hairline)
        })
        .child(
            div()
                .text_size(px(11.))
                .line_height(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.fg_tertiary)
                .child(label),
        )
}

fn editing_tab(
    input: &Entity<TextInput>,
    prod: bool,
    p: Palette,
    mono: SharedString,
) -> gpui_kit::Div {
    let mut tab = h_flex()
        .relative()
        .h(px(TAB_H_PX))
        .px(px(TAB_PAD_X_PX))
        .gap(px(TAB_GAP_PX))
        .items_center()
        .rounded_t(px(6.))
        .font_family(mono)
        .text_size(px(TAB_TEXT_PX))
        .line_height(px(TAB_LINE_PX))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.fg_primary)
        .bg(p.bg_canvas);
    if prod {
        tab = tab.child(
            div()
                .size(px(TAB_DOT_PX))
                .rounded(px(999.))
                .bg(p.prod_dot)
                .flex_shrink_0(),
        );
    }
    tab.child(
        div()
            .min_w(px(72.))
            .child(inline_field(input, p, TAB_LINE_PX)),
    )
}

/// The field sits on the label's line. The ring is drawn outside that line.
fn inline_field(input: &Entity<TextInput>, p: Palette, line: f32) -> gpui_kit::Div {
    div()
        .relative()
        .w_full()
        .h(px(line))
        .min_w(px(0.))
        .child(
            div()
                .absolute()
                .top(px(-4.))
                .bottom(px(-4.))
                .left(px(-4.))
                .right(px(-2.))
                .rounded(px(4.))
                .border_1()
                .border_color(p.accent_ring)
                .bg(p.bg_canvas.opacity(0.35)),
        )
        .child(input.clone())
}

fn lined_field(input: &Entity<TextInput>, p: Palette) -> gpui_kit::Div {
    div()
        .h(px(28.))
        .w_full()
        .min_w(px(0.))
        .px(px(8.))
        .rounded(px(6.))
        .border_1()
        .border_color(p.border_control)
        .bg(p.bg_surface)
        .child(input.clone())
}

fn file_title(name: &str, suffix: &str, p: Palette) -> gpui_kit::Div {
    let mut title = h_flex()
        .gap(px(6.))
        .items_center()
        .min_w(px(0.))
        .child(div().min_w(px(0.)).truncate().child(name.to_string()));
    if !suffix.is_empty() {
        title = title.child(
            div()
                .max_w(px(96.))
                .min_w(px(0.))
                .truncate()
                .text_size(px(11.))
                .line_height(px(14.))
                .font_weight(FontWeight::NORMAL)
                .text_color(p.fg_tertiary)
                .child(suffix.to_string()),
        );
    }
    title
}

fn search_field(
    input: &Entity<TextInput>,
    clear_id: &'static str,
    p: Palette,
    window: &Window,
    cx: &mut Context<EnvyApp>,
) -> gpui_kit::Div {
    let empty = input.read(cx).is_empty();
    let focused = input.read(cx).focus_handle(cx).is_focused(window);
    let clear_input = input.clone();
    h_flex()
        .h(px(SEARCH_FIELD_H_PX))
        .w_full()
        .px(px(8.))
        .gap(px(6.))
        .items_center()
        .text_size(px(SEARCH_TEXT_PX))
        .line_height(px(SEARCH_LINE_PX))
        .rounded(px(6.))
        .border_1()
        .border_color(if focused { p.accent } else { p.border_control })
        .bg(p.bg_surface)
        .child(
            Icon::new(IconName::Search)
                .size(px(13.))
                .text_color(p.fg_tertiary),
        )
        .child(div().flex_1().h_full().min_w(px(0.)).child(input.clone()))
        .when(!empty, |row| {
            row.child(
                div()
                    .id(clear_id)
                    .w(px(16.))
                    .h(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(p.fg_tertiary)
                    .hover(|style| style.text_color(p.fg_primary))
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        clear_input.update(cx, |input, cx| input.set_value("", window, cx));
                        window.focus(&clear_input.focus_handle(cx), cx);
                    })
                    .child(Icon::new(IconName::X).size(px(12.))),
            )
        })
}

fn toolbar_icon_button(
    id: impl Into<SharedString>,
    icon: IconName,
    tip: impl Into<SharedString>,
    active: bool,
    disabled: bool,
    p: Palette,
) -> Button {
    let mut btn = Button::new(id.into())
        .ghost()
        .small()
        .compact()
        .icon(icon)
        .tooltip(tip)
        .disabled(disabled)
        .rounded(px(6.));
    if active {
        btn = btn
            .bg(p.accent_subtle)
            .border_1()
            .border_color(p.accent_ring)
            .text_color(p.accent);
    } else if disabled {
        btn = btn.text_color(p.fg_tertiary.opacity(0.38)).cursor_default();
    } else {
        btn = btn.text_color(p.fg_secondary);
    }
    btn
}

fn row_icon_button(
    id: impl Into<SharedString>,
    icon: IconName,
    tip: impl Into<SharedString>,
) -> Button {
    Button::new(id.into())
        .ghost()
        .xsmall()
        .compact()
        .icon(icon)
        .tooltip(tip)
        .rounded(px(4.))
}

fn app_mark() -> Arc<gpui_kit::Image> {
    static MARK: OnceLock<Arc<gpui_kit::Image>> = OnceLock::new();
    MARK.get_or_init(|| {
        Arc::new(gpui_kit::Image::from_bytes(
            gpui_kit::ImageFormat::Png,
            include_bytes!("../../../assets/icon/envy-icon-32.png").to_vec(),
        ))
    })
    .clone()
}

fn files_found_label(count: usize) -> String {
    if count == 1 {
        "1 file".into()
    } else {
        format!("{count} files")
    }
}

fn hover_tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |_, cx| cx.new(|_| Tooltip::new(text.clone())).into()
}
