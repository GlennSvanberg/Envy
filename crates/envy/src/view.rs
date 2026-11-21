//! The Envy window.
//!
//! Projects on the left, the open file on the right. Values stay in memory
//! and are not written to the status line.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use envy_core::discover::{
    default_file_index, is_example_filename, is_prod_filename, resolve_file, resolve_project,
};
use envy_core::document::Node;
use envy_core::paste::{PasteChange, PastePlan};
use envy_core::{
    attach_git_identity, cache_from_projects, diff, fill_key_index, from_example, is_env_key,
    load_index_cache, plan_paste, projects_from_cache, save_index_cache, save_text, scan_paths,
    sidebar_rank, upsert_file, Config, Deeplink, EnvDocument, EnvFile, FoundEnv, OpenFile, Project,
    ScanError, ThemeSetting,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Disableable, Icon, Selectable, Sizable, Theme, ThemeMode, TitleBar,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, size, AnyView, AnyWindowHandle, App, Bounds, ClipboardItem, Context, Entity,
    FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton, PathPromptOptions, ScrollHandle,
    SharedString, Subscription, Task, Window, WindowBounds, WindowOptions,
};

use crate::actions::*;
use crate::hotkey_os;
use crate::instance::Instance;
use crate::theme::{
    self, Palette, COL_HEADER_H, DIFF_GUTTER_W, DIFF_HEADER_H, DIFF_KEY_COL_W, DIFF_ROW_H,
    FOOTER_H, KEY_ACTIONS_W, KEY_COL_W, KEY_ROW_H, MASK_MAX_DOTS, SIDEBAR_HEADER_H, SIDEBAR_ROW_H,
    SIDEBAR_W, TAB_BAR_H, TOOLBAR_H,
};

const CLEAR_AFTER: Duration = Duration::from_secs(30);

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
    key: Option<Entity<InputState>>,
    value: Entity<InputState>,
    node: Option<usize>,
    kind: EditKind,
}

enum Overlay {
    None,
    Settings,
    AddFolder,
    Paste(PastePlan),
    Shortcuts,
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
    cursor: usize,
    revealed: HashSet<String>,
    show_all: bool,
    editor: Option<Editor>,
    edit_subs: Vec<Subscription>,
    overlay: Overlay,
    folder_input: Entity<InputState>,
    hotkey_input: Entity<InputState>,
    input_subs: Vec<Subscription>,
    project_scroll: ScrollHandle,
    key_scroll: ScrollHandle,
    diff_scroll: ScrollHandle,
    project_filter: Entity<InputState>,
    filter_query: String,
    key_filter: Entity<InputState>,
    key_query: String,
    diff_open: bool,
    diff_cursor: usize,
    diff_left: Option<usize>,
    diff_right: Option<usize>,
    diff_left_doc: Option<EnvDocument>,
    diff_right_doc: Option<EnvDocument>,
    last_dark: Option<bool>,
    status: String,
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
        epoch: u64,
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
    Quit,
}

impl EnvyApp {
    pub fn new(
        instance: Option<Instance>,
        launch: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let folder_input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder path"));
        let hotkey_input = cx.new(|cx| InputState::new(window, cx).placeholder("ctrl+alt+e"));
        let project_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Find a project"));
        let key_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Find a key"));
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
            key_filter,
            key_query: String::new(),
            diff_open: false,
            diff_cursor: 0,
            diff_left: None,
            diff_right: None,
            diff_left_doc: None,
            diff_right_doc: None,
            last_dark: None,
            status: "Starting".into(),
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
                if let InputEvent::PressEnter { .. } = event {
                    let path = state.read(cx).value().to_string();
                    this.commit_folder(path, cx);
                }
            },
        ));
        self.input_subs.push(cx.subscribe_in(
            &self.project_filter,
            window,
            |this, state, event, window, cx| match event {
                InputEvent::Change => {
                    this.filter_query = state.read(cx).value().to_string();
                    this.scroll_project_into_view();
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    this.open_filtered_project(window, cx);
                }
                InputEvent::Focus | InputEvent::Blur => {}
            },
        ));
        self.input_subs.push(cx.subscribe_in(
            &self.key_filter,
            window,
            |this, state, event, window, cx| match event {
                InputEvent::Change => {
                    this.key_query = state.read(cx).value().to_string();
                    let len = this.visible_entry_nodes().len();
                    if this.cursor >= len {
                        this.cursor = 0;
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    this.zone = Zone::Keys;
                    this.focus_self(window, cx);
                    cx.notify();
                }
                InputEvent::Focus | InputEvent::Blur => {}
            },
        ));
        self.input_subs.push(cx.subscribe_in(
            &self.hotkey_input,
            window,
            |this, state, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let spec = state.read(cx).value().to_string();
                    this.commit_hotkey(spec, window, cx);
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

    fn visible_entry_nodes(&self) -> Vec<usize> {
        let nodes = self.entry_nodes();
        let query = self.key_query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return nodes;
        }
        let Some(open) = &self.open else {
            return nodes;
        };
        nodes
            .into_iter()
            .filter(|node| match open.document.nodes.get(*node) {
                Some(Node::Entry(entry)) => fuzzy_hit(&entry.key, &query),
                _ => false,
            })
            .collect()
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
                query.is_empty()
                    || fuzzy_hit(&project.name, &query)
                    || fuzzy_hit(&project.root.display().to_string(), &query)
                    || fuzzy_hit(&project.branch, &query)
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
        let Some(position) = self
            .visible_project_indexes()
            .iter()
            .position(|visible| *visible == index)
        else {
            return;
        };
        self.project_scroll.scroll_to_item(position);
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
        match self.zone {
            Zone::Tabs => self.move_tab(-1, cx),
            Zone::Keys | Zone::Projects => self.focus_sidebar(window, cx),
        }
    }

    fn on_right(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            self.commit_focused_field(window, cx);
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

    fn step_out_of_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.filter_focused(Zone::Keys, window, cx) {
            self.zone = Zone::Keys;
            self.focus_self(window, cx);
            cx.notify();
            return;
        }
        self.zone = Zone::Projects;
        let visible = self.visible_project_indexes();
        let index = self
            .project_ix
            .filter(|index| visible.contains(index))
            .or_else(|| visible.first().copied());
        if let Some(index) = index {
            if self.project_ix == Some(index) {
                self.focus_self(window, cx);
                cx.notify();
            } else {
                self.select_project(index, false, window, cx);
            }
        } else {
            self.focus_self(window, cx);
            cx.notify();
        }
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
        let visible = self.visible_project_indexes();
        let index = self
            .project_ix
            .filter(|index| visible.contains(index))
            .or_else(|| visible.first().copied());
        if let Some(index) = index {
            self.remember_project(index, cx);
            self.select_project(index, true, window, cx);
        }
    }

    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.overlay, Overlay::None) || self.editor.is_some() {
            return;
        }
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
            return;
        };
        if self.open.as_ref().is_some_and(|open| open.path == path) {
            return;
        }
        if let Some(open) = self.open.as_ref() {
            if open.is_dirty() {
                self.enqueue_save(cx);
            }
        }
        self.io_epoch = self.io_epoch.wrapping_add(1);
        let epoch = self.io_epoch;
        self.status = "Opening…".into();
        self.io_queue.push_back(IoJob::Load { epoch, path });
        self.pump_io(cx);
    }

    fn enqueue_save(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &self.open else {
            return;
        };
        if !open.is_dirty() {
            return;
        }
        self.save_seq = self.save_seq.wrapping_add(1);
        self.io_queue.push_back(IoJob::Save {
            seq: self.save_seq,
            epoch: self.io_epoch,
            path: open.path.clone(),
            text: open.document.serialize(),
        });
        self.status = "Saving…".into();
        self.pump_io(cx);
    }

    fn pump_io(&mut self, cx: &mut Context<Self>) {
        if self.io_running {
            return;
        }
        let Some(job) = self.io_queue.pop_front() else {
            return;
        };
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
        match done {
            IoDone::Saved {
                seq,
                epoch,
                path,
                text,
                result,
            } => match result {
                Ok(mtime) => {
                    if self.save_seq == seq && self.io_epoch == epoch {
                        if let Some(open) = self.open.as_mut() {
                            if open.path == path {
                                open.note_saved(text, mtime);
                            }
                        }
                        if self.status == "Saving…" {
                            self.status = "Saved.".into();
                        }
                    }
                }
                Err(err) => {
                    self.status = format!("Save failed: {err}");
                    self.io_queue.retain(|job| !matches!(job, IoJob::Quit));
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
                        if self.diff_open {
                            self.load_diff(cx);
                        }
                        if let Some(key) = self.pending_key.take() {
                            self.jump_to_key(&key);
                            self.status = format!("Jumped to {key}");
                        } else if self.status == "Opening…" || self.status == "Saving…" {
                            self.status.clear();
                        }
                        let _ = path;
                    }
                    Err(err) => {
                        self.open = None;
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
                        self.cursor = 0;
                        self.sync_keys();
                        self.diff_stale = true;
                        self.status = "Reloaded from disk.".into();
                    }
                    Err(err) => self.status = format!("Reload failed: {err}"),
                }
            }
            IoDone::Quit => cx.quit(),
        }
        cx.notify();
    }

    fn schedule_disk_check(&mut self, cx: &mut Context<Self>) {
        if self.disk_check || self.last_disk.elapsed() < Duration::from_millis(300) {
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
                    view.status = "Changed on disk. R reloads, Esc keeps yours.".into();
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

    fn side_name(&self, side: DiffSide) -> String {
        self.side_path(side)
            .and_then(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "Choose a file".into())
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

    fn close_editor(&mut self) {
        self.editor = None;
        self.edit_subs.clear();
    }

    fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        self.focus_self(window, cx);
        cx.notify();
    }

    /// Save if needed, then quit. Returns false while a save is still running
    /// so the window stays up until the write finishes.
    pub fn close_requested(&mut self, cx: &mut Context<Self>) -> bool {
        if self.open.as_ref().is_some_and(|open| open.is_dirty()) {
            self.enqueue_save(cx);
            self.io_queue.push_back(IoJob::Quit);
            self.pump_io(cx);
            false
        } else {
            cx.quit();
            true
        }
    }

    fn save_open(&mut self, cx: &mut Context<Self>) {
        self.enqueue_save(cx);
        cx.notify();
    }

    fn mark_edited(&mut self) {
        self.io_epoch = self.io_epoch.wrapping_add(1);
        self.io_queue
            .retain(|job| !matches!(job, IoJob::Load { .. }));
        self.diff_stale = true;
        self.sync_keys();
        self.status = "Edited. Ctrl+S saves.".into();
    }

    fn sync_keys(&mut self) {
        let Some(open) = &self.open else {
            return;
        };
        let keys = open.document.key_names();
        let path = open.path.clone();
        let Some(project) = self.project_ix.and_then(|ix| self.projects.get_mut(ix)) else {
            return;
        };
        if let Some(file) = project.files.iter_mut().find(|file| file.path == path) {
            file.keys = keys;
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
        let nodes = self.entry_nodes();
        let Some(open) = &self.open else {
            return;
        };
        if let Some(position) = nodes
            .iter()
            .position(|node| match open.document.nodes.get(*node) {
                Some(Node::Entry(entry)) => entry.key == key,
                _ => false,
            })
        {
            self.cursor = position;
            self.key_scroll.scroll_to_item(position);
            self.zone = Zone::Keys;
        }
    }

    fn move_delta(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        match self.zone {
            Zone::Projects => {
                let visible = self.visible_project_indexes();
                if visible.is_empty() {
                    if delta < 0 {
                        self.focus_search(window, cx);
                    }
                    return;
                }
                let current = self
                    .project_ix
                    .and_then(|index| visible.iter().position(|item| *item == index))
                    .map(|position| position as isize)
                    .unwrap_or(-1);
                if delta < 0 && current <= 0 {
                    self.focus_search(window, cx);
                    return;
                }
                let next_pos = (current + delta).clamp(0, visible.len() as isize - 1) as usize;
                self.select_project(visible[next_pos], false, window, cx);
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
                let len = self.visible_entry_nodes().len() as isize;
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
                self.key_scroll.scroll_to_item(next);
                cx.notify();
            }
        }
    }

    fn move_file(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
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
        match &self.overlay {
            Overlay::Paste(_) => {
                self.apply_overlay_paste(window, cx);
                return;
            }
            Overlay::Shortcuts => {
                self.close_overlay(window, cx);
                return;
            }
            Overlay::AddFolder => return,
            Overlay::Settings => return,
            Overlay::None => {}
        }
        match self.zone {
            Zone::Projects => {
                let visible = self.visible_project_indexes();
                let index = self
                    .project_ix
                    .filter(|index| visible.contains(index))
                    .or_else(|| visible.first().copied());
                if let Some(index) = index {
                    self.remember_project(index, cx);
                    self.select_project(index, true, window, cx);
                }
            }
            Zone::Tabs => self.activate_tab(self.tab_cursor, false, window, cx),
            Zone::Keys => self.begin_value_edit(window, cx),
        }
    }

    fn commit_focused_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (tag, key_focused, key_text, value_text) = {
            let Some(editor) = self.editor.as_ref() else {
                return;
            };
            let key_focused = editor
                .key
                .as_ref()
                .is_some_and(|input| input.focus_handle(cx).is_focused(window));
            let key_text = editor
                .key
                .as_ref()
                .map(|input| input.read(cx).value().to_string());
            let value_text = editor.value.read(cx).value().to_string();
            (editor.kind_tag(), key_focused, key_text, value_text)
        };
        match (tag, key_focused) {
            (EditTag::New, true) => self.focus_value_editor(window, cx),
            (EditTag::Rename, true) => {
                self.commit_editor_key(key_text.unwrap_or_default(), window, cx);
            }
            _ => self.commit_editor(Some(value_text), window, cx),
        }
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.overlay, Overlay::None) {
            self.close_overlay(window, cx);
            return;
        }
        if self.editor.is_some() {
            self.close_editor();
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
        if self.open.is_none() {
            self.status = "Open a file first.".into();
            cx.notify();
            return;
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
        self.close_editor();
        let key_input = key.map(|text| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(text)
                    .placeholder("KEY")
            })
        });
        let value_text = value.unwrap_or_default();
        let value_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(value_text)
                .placeholder("value")
        });
        if let Some(key_input) = &key_input {
            self.edit_subs.push(cx.subscribe_in(
                key_input,
                window,
                |this, input, event, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        let rename = matches!(
                            this.editor.as_ref().map(|editor| editor.kind_tag()),
                            Some(EditTag::Rename)
                        );
                        if rename {
                            let key = input.read(cx).value().to_string();
                            this.commit_editor_key(key, window, cx);
                        } else {
                            this.focus_value_editor(window, cx);
                        }
                    }
                },
            ));
        }
        self.edit_subs.push(cx.subscribe_in(
            &value_input,
            window,
            |this, state, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let text = state.read(cx).value().to_string();
                    this.commit_editor(Some(text), window, cx);
                }
            },
        ));
        let focus_key = key_input.is_some() && !matches!(kind, EditKind::Value);
        self.editor = Some(Editor {
            key: key_input,
            value: value_input,
            node,
            kind,
        });
        if focus_key {
            self.focus_key_editor(window, cx);
        } else {
            self.focus_value_editor(window, cx);
        }
        cx.notify();
    }

    fn focus_key_editor(&self, window: &mut Window, cx: &mut App) {
        if let Some(key) = self.editor.as_ref().and_then(|editor| editor.key.as_ref()) {
            let handle = key.focus_handle(cx);
            window.focus(&handle, cx);
        }
    }

    fn focus_value_editor(&self, window: &mut Window, cx: &mut App) {
        if let Some(editor) = &self.editor {
            let handle = editor.value.focus_handle(cx);
            window.focus(&handle, cx);
        }
    }

    fn commit_editor_key(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        let node = self.editor.as_ref().and_then(|editor| editor.node);
        self.close_editor();
        self.write_key(node, key);
        self.focus_self(window, cx);
        cx.notify();
    }

    fn commit_editor(
        &mut self,
        value_text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let kind = editor.kind_tag();
        let node = editor.node;
        let key_text = editor
            .key
            .as_ref()
            .map(|input| input.read(cx).value().to_string());
        let value_text = value_text.unwrap_or_else(|| {
            self.editor
                .as_ref()
                .map(|editor| editor.value.read(cx).value().to_string())
                .unwrap_or_default()
        });
        self.close_editor();
        match kind {
            EditTag::Value => self.write_value(node, value_text),
            EditTag::Rename => self.write_key(node, key_text.unwrap_or_default()),
            EditTag::New => self.write_new(key_text.unwrap_or_default(), value_text),
        }
        self.focus_self(window, cx);
        cx.notify();
    }

    fn write_value(&mut self, node: Option<usize>, value: String) {
        let Some(node) = node else {
            return;
        };
        let unchanged = self
            .open
            .as_ref()
            .and_then(|open| match open.document.nodes.get(node) {
                Some(Node::Entry(entry)) => Some(entry.value == value),
                _ => None,
            });
        if unchanged == Some(true) {
            return;
        }
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.mutate(|document| {
            if let Some(Node::Entry(entry)) = document.nodes.get_mut(node) {
                entry.set_value(value);
            }
        });
        self.mark_edited();
    }

    fn write_key(&mut self, node: Option<usize>, key: String) {
        let key = key.trim().to_string();
        if !is_env_key(&key) {
            self.status = "Keys start with a letter or underscore.".into();
            return;
        }
        let Some(node) = node else {
            return;
        };
        let taken = self.open.as_ref().is_some_and(|open| {
            open.document
                .entries()
                .any(|(index, entry)| index != node && entry.key == key)
        });
        if taken {
            self.status = format!("{key} is already in this file.");
            return;
        }
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.mutate(|document| {
            if let Some(Node::Entry(entry)) = document.nodes.get_mut(node) {
                entry.set_key(&key);
            }
        });
        self.mark_edited();
        self.jump_to_key(&key);
        self.status = format!("Renamed to {key}. Ctrl+S saves.");
    }

    fn write_new(&mut self, key: String, value: String) {
        let key = key.trim().to_string();
        if !is_env_key(&key) {
            self.status = "Keys start with a letter or underscore.".into();
            return;
        }
        let exists = self
            .open
            .as_ref()
            .is_some_and(|open| open.document.contains_key(&key));
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.mutate(|document| {
            if exists {
                document.set_value(&key, &value);
            } else {
                document.push_entry(key.clone(), value);
            }
        });
        self.mark_edited();
        self.jump_to_key(&key);
        self.status = if exists {
            format!("Updated {key}. Ctrl+S saves.")
        } else {
            format!("Added {key}. Ctrl+S saves.")
        };
    }

    fn copy_selected(&mut self, cx: &mut Context<Self>) {
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
        self.status = if self.config.settings.clear_clipboard {
            "Copied. Clipboard clears in 30s.".into()
        } else {
            "Copied.".into()
        };
        cx.notify();
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

    fn ask_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        self.mark_edited();
        self.status = format!("Removed {key}. Ctrl+Z brings it back.");
        self.focus_self(window, cx);
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if !open.undo() {
            self.status = "Nothing to undo.".into();
            cx.notify();
            return;
        }
        self.sync_keys();
        self.status = if self.open.as_ref().is_some_and(|open| open.is_dirty()) {
            "Undone. Ctrl+S saves.".into()
        } else {
            "Undone.".into()
        };
        cx.notify();
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if !open.redo() {
            return;
        }
        self.sync_keys();
        self.status = if self.open.as_ref().is_some_and(|open| open.is_dirty()) {
            "Redone. Ctrl+S saves.".into()
        } else {
            "Redone.".into()
        };
        cx.notify();
    }

    fn reload_external(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &self.open else {
            return;
        };
        if !open.external_change {
            return;
        }
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

    fn new_from_example(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.project_ix.and_then(|ix| self.projects.get(ix)) else {
            self.status = "Open a project first.".into();
            cx.notify();
            return;
        };
        let Some(example) = project
            .files
            .iter()
            .find(|file| is_example_filename(&file.name))
        else {
            self.status = "This project has no .env.example.".into();
            cx.notify();
            return;
        };
        let example_path = example.path.clone();
        let dest = example
            .path
            .parent()
            .unwrap_or(project.root.as_path())
            .join(".env.local");
        self.status = "Creating .env.local…".into();
        cx.notify();
        cx.spawn(async move |this, async_cx| {
            let created = async_cx
                .background_executor()
                .spawn(async move {
                    if dest.exists() {
                        return Err(".env.local already exists.".to_string());
                    }
                    let loaded = OpenFile::load(&example_path)
                        .map_err(|err| format!("Could not read example: {err}"))?;
                    let document = from_example(&loaded.document);
                    OpenFile::create(&dest, document)
                        .map_err(|err| format!("Could not create file: {err}"))?;
                    Ok(dest)
                })
                .await;
            let _ = this.update(async_cx, |view, cx| {
                match created {
                    Ok(dest) => {
                        view.pending_file = Some(dest);
                        view.status = "Created .env.local from the example.".into();
                        view.begin_scan(false, cx);
                    }
                    Err(err) => view.status = err,
                }
                cx.notify();
            });
        })
        .detach();
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
        let Overlay::Paste(plan) = &self.overlay else {
            return;
        };
        let plan = plan.clone();
        self.overlay = Overlay::None;
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.apply_paste(&plan);
        self.mark_edited();
        self.status = format!(
            "Pasted {} added, {} overwritten. Ctrl+S saves.",
            plan.added(),
            plan.overwritten()
        );
        self.focus_self(window, cx);
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
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
        match key {
            "tab" if self.editor.is_some() => {
                self.commit_focused_field(window, cx);
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
            "down"
                if self.editor.is_none()
                    && matches!(self.overlay, Overlay::None)
                    && (self.filter_focused(Zone::Projects, window, cx)
                        || self.filter_focused(Zone::Keys, window, cx)) =>
            {
                self.step_out_of_search(window, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "escape" => {
                self.cancel(window, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "enter" if mods.shift && self.editor.is_none() && self.zone == Zone::Tabs => {
                self.toggle_compare_tab(self.tab_cursor, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "enter" if self.capture_enter() => {
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
            .keys
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
        .map(|file| file.keys.clone());
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

fn block_paste(
    entity: gpui_kit::WeakEntity<EnvyApp>,
    focus: FocusHandle,
) -> impl Fn(&gpui_kit::ClipboardItem, &mut Window, &mut App) -> bool + 'static {
    move |item, window, cx| {
        let text = item.text();
        let handled = entity
            .update(cx, |this, _cx| {
                text.map(|text| this.stage_paste(text)).unwrap_or(false)
            })
            .unwrap_or(false);
        if handled {
            window.focus(&focus, cx);
        }
        handled
    }
}

fn write_pairs(document: &mut EnvDocument, pairs: &[(String, String)]) {
    for (key, value) in pairs {
        if document.set_value(key, value) == 0 {
            document.push_entry(key.clone(), value.clone());
        }
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

impl Render for EnvyApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::for_mode(cx.theme().mode.is_dark());
        let mono = cx.theme().mono_font_family.clone();
        let ui_font = cx.theme().font_family.clone();

        let sidebar = self.render_sidebar(p, window, cx);
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
            let footer = self.render_footer(p, mono.clone());
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
            let toolbar = self.render_toolbar(p, missing.len(), cx);
            let body = if self.diff_open {
                self.render_diff_view(p, mono.clone(), cx)
            } else {
                self.render_key_table(p, mono.clone(), window, cx)
            };
            let footer = self.render_footer(p, mono.clone());
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
            .on_action(cx.listener(|this, _: &Confirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.cancel(window, cx)))
            .on_action(cx.listener(|this, _: &CopyValue, _, cx| this.copy_selected(cx)))
            .on_action(cx.listener(|this, _: &ToggleReveal, _, cx| this.toggle_reveal(cx)))
            .on_action(cx.listener(|this, _: &RevealAll, _, cx| this.toggle_show_all(cx)))
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
            .on_action(cx.listener(|this, _: &NewFromExample, _, cx| this.new_from_example(cx)))
            .on_action(
                cx.listener(|this, _: &AddFolder, window, cx| this.open_add_folder(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenSettings, window, cx| this.open_settings(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ReloadExternal, _, cx| this.reload_external(cx)))
            .on_action(cx.listener(|this, _: &KeepMine, window, cx| this.cancel(window, cx)))
            .on_action(cx.listener(|this, _: &PasteBlock, _, cx| this.paste_from_clipboard(cx)))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.on_key(event, window, cx);
            }))
            .child(sidebar)
            .child(center);

        if !matches!(self.overlay, Overlay::None) {
            root = root.child(self.overlay_view(p, mono, window, cx));
        }

        v_flex()
            .size_full()
            .bg(p.bg_canvas)
            .text_color(p.fg_primary)
            .child(
                TitleBar::new().child(
                    h_flex()
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
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.fg_primary)
                                .child("Envy"),
                        ),
                ),
            )
            .child(root.flex_1().min_h(px(0.)).w_full())
    }
}

impl EnvyApp {
    fn render_sidebar(
        &self,
        p: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let project_rows = self.projects.clone();
        let visible_projects = self.visible_project_indexes();
        let project_ix = self.project_ix;
        let zone = self.zone;
        let list_focused =
            zone == Zone::Projects && !self.filter_focused(Zone::Projects, window, cx);
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

        let search_box = div().px(px(8.)).pb(px(8.)).flex_shrink_0().child(
            Input::new(&self.project_filter)
                .small()
                .cleanable(true)
                .prefix(
                    Icon::new(IconName::Search)
                        .size(px(13.))
                        .text_color(p.fg_tertiary),
                ),
        );

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

        for (visible_ix, index) in visible_projects.iter().copied().enumerate() {
            let project = &project_rows[index];
            let selected = project_ix == Some(index);
            let favorite = self.config.is_favorite(&project.root);
            let branch = display_branch(&project.branch).map(str::to_string);
            let entity = entity.clone();
            let star_entity = entity.clone();
            let group = SharedString::from(format!("project-row-{index}"));

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
                .mx(px(4.))
                .px(px(8.))
                .h(SIDEBAR_ROW_H)
                .flex_shrink_0()
                .gap(px(8.))
                .rounded(px(6.))
                .items_center()
                .overflow_hidden()
                .cursor_pointer()
                .tooltip(hover_tip(format!(
                    "{} · {}",
                    project.name,
                    project.root.display()
                )))
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

            row = row.child(
                div()
                    .size(px(6.))
                    .rounded(px(999.))
                    .flex_shrink_0()
                    .when(project.missing_vs_example, |dot| dot.bg(p.warning_dot)),
            );

            let mut text_col = v_flex()
                .flex_1()
                .min_w(px(0.))
                .justify_center()
                .gap(px(2.))
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
                        .text_color(if selected && list_focused {
                            p.accent
                        } else {
                            p.fg_primary
                        })
                        .child(project.name.clone()),
                );

            if let Some(branch_name) = branch {
                text_col = text_col.child(
                    h_flex()
                        .items_center()
                        .gap(px(4.))
                        .min_w(px(0.))
                        .child(
                            Icon::new(IconName::GitBranch)
                                .size(px(11.))
                                .text_color(p.fg_tertiary),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(px(11.))
                                .line_height(px(14.))
                                .text_color(p.fg_tertiary)
                                .truncate()
                                .child(branch_name),
                        ),
                );
            }

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
            .w(SIDEBAR_W)
            .flex_shrink_0()
            .h_full()
            .bg(p.bg_chrome)
            .text_color(p.fg_primary)
            .border_r_1()
            .border_color(p.border_hairline)
            .child(header)
            .when(!self.config.folders.is_empty(), |sidebar| {
                sidebar.child(search_box)
            })
            .child(
                div()
                    .id("project-scroll-host")
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .mx(px(4.))
                    .mb(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if list_focused {
                        p.accent_ring
                    } else {
                        p.bg_chrome.opacity(0.)
                    })
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
        let dirty = self.open.as_ref().is_some_and(|open| open.is_dirty());
        let can_compare = file_tabs.len() >= 2;

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
            let is_open_dirty = file_ix == Some(index) && dirty;
            let cursor_tab = zone == Zone::Tabs && self.tab_cursor == index;
            let entity = cx.entity();
            let name = file.name.clone();

            let mut tip_parts = vec![name.clone()];
            if file.prod {
                tip_parts.push("Production".into());
            }
            if is_open_dirty {
                tip_parts.push("Unsaved changes".into());
            } else if file.missing_vs_example {
                tip_parts.push("Missing keys vs .env.example".into());
            }
            if can_compare && !self.diff_open && file_ix != Some(index) {
                tip_parts.push("Ctrl+click to compare".into());
            }
            let tip = tip_parts.join(" · ");

            let mut tab = h_flex()
                .id(SharedString::from(format!("file-{index}")))
                .relative()
                .h(px(33.))
                .px(px(12.))
                .gap(px(6.))
                .items_center()
                .rounded_t(px(6.))
                .cursor_pointer()
                .font_family(mono.clone())
                .text_size(px(12.5))
                .line_height(px(16.))
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
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    entity.update(cx, |this, cx| {
                        this.zone = Zone::Tabs;
                        this.tab_cursor = index;
                        if event.modifiers.control || event.modifiers.shift {
                            this.toggle_compare_tab(index, cx);
                        } else {
                            this.activate_tab(index, false, window, cx);
                        }
                    });
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
                        .size(px(6.))
                        .rounded(px(999.))
                        .bg(p.prod_dot)
                        .flex_shrink_0(),
                );
            }

            tab = tab.child(div().truncate().child(name));

            if is_open_dirty {
                tab = tab.child(
                    div()
                        .size(px(6.))
                        .rounded(px(999.))
                        .bg(p.warning_dot)
                        .flex_shrink_0(),
                );
            } else if file.missing_vs_example {
                tab = tab.child(
                    div()
                        .size(px(5.))
                        .rounded(px(999.))
                        .bg(p.warning_dot.opacity(0.65))
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

        tabs
    }

    fn render_toolbar(
        &self,
        p: Palette,
        missing_count: usize,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let entity = cx.entity();
        let show_all = self.show_all;
        let diff_open = self.diff_open;
        let dirty = self.open.as_ref().is_some_and(|open| open.is_dirty());
        let can_undo = self.open.as_ref().is_some_and(|open| open.can_undo());
        let can_redo = self.open.as_ref().is_some_and(|open| open.can_redo());
        let has_open = self.open.is_some();
        let file_count = self.file_count();
        let has_example = self
            .project_ix
            .and_then(|ix| self.projects.get(ix))
            .is_some_and(|project| project.files.iter().any(|f| is_example_filename(&f.name)));

        let left_side = if diff_open {
            let mut left = h_flex().items_center().gap(px(8.)).flex_1().min_w(px(0.));
            left = left.child(
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
            );
            if let Some(report) = &self.diff_report {
                let mut different = 0usize;
                let mut only_left = 0usize;
                let mut only_right = 0usize;
                for row in &report.rows {
                    match row.kind {
                        diff::DiffKind::Different => different += 1,
                        diff::DiffKind::OnlyLeft => only_left += 1,
                        diff::DiffKind::OnlyRight => only_right += 1,
                        diff::DiffKind::Same => {}
                    }
                }
                if different == 0 && only_left == 0 && only_right == 0 {
                    left = left.child(
                        div()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(px(999.))
                            .bg(p.accent_subtle)
                            .text_size(px(11.5))
                            .line_height(px(16.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.accent)
                            .child("Files match"),
                    );
                } else {
                    if different > 0 {
                        left = left.child(summary_pill(
                            format!("{different} differ"),
                            p.warning,
                            p.warning_bg,
                            p.warning_border,
                        ));
                    }
                    if only_left > 0 {
                        left = left.child(summary_pill(
                            format!("{only_left} left only"),
                            p.accent,
                            p.accent_subtle,
                            p.accent_ring,
                        ));
                    }
                    if only_right > 0 {
                        left = left.child(summary_pill(
                            format!("{only_right} right only"),
                            p.danger,
                            p.danger_bg,
                            p.danger_border,
                        ));
                    }
                }
            }
            if file_count >= 3 {
                left = left.child(
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
                );
            }
            left
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
                .child(
                    div().w(px(220.)).flex_shrink_0().child(
                        Input::new(&self.key_filter).small().cleanable(true).prefix(
                            Icon::new(IconName::Search)
                                .size(px(13.))
                                .text_color(p.fg_tertiary),
                        ),
                    ),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(p.fg_tertiary)
                        .whitespace_nowrap()
                        .child(count_label),
                );

            if missing_count > 0 {
                let compare_entity = entity.clone();
                left = left.child(
                    h_flex()
                        .id("missing-vs-example-pill")
                        .h(px(24.))
                        .px(px(8.))
                        .gap(px(5.))
                        .items_center()
                        .rounded(px(999.))
                        .bg(p.warning_bg)
                        .border_1()
                        .border_color(p.warning_border)
                        .text_size(px(11.5))
                        .line_height(px(15.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.warning)
                        .cursor_pointer()
                        .hover(move |style| style.bg(p.warning.opacity(0.16)))
                        .tooltip(hover_tip("Compare with .env.example"))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            compare_entity.update(cx, |this, cx| this.compare_with_example(cx));
                        })
                        .child(
                            Icon::new(IconName::Info)
                                .size(px(12.))
                                .text_color(p.warning),
                        )
                        .child(format!("{missing_count} missing vs .env.example")),
                );
            }

            left
        };

        let right_side = h_flex()
            .items_center()
            .gap(px(4.))
            .flex_shrink_0()
            .child(
                toolbar_icon_button("tb-new", IconName::Plus, "New key", false, !has_open, p)
                    .tooltip_with_action("New key", &NewKey, Some("Envy"))
                    .on_click({
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| this.begin_new_key(window, cx));
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
            .child(
                toolbar_icon_button("tb-save", IconName::Save, "Save", dirty, !dirty, p)
                    .tooltip_with_action("Save", &Save, Some("Envy"))
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.save_open(cx);
                                cx.notify();
                            });
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
                    show_all,
                    false,
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
            .child(
                toolbar_icon_button(
                    "tb-example",
                    IconName::FilePlus,
                    "New .env.local from example",
                    false,
                    !has_example,
                    p,
                )
                .tooltip_with_action("New .env.local from example", &NewFromExample, Some("Envy"))
                .on_click({
                    let entity = entity.clone();
                    move |_, _, cx| {
                        entity.update(cx, |this, cx| this.new_from_example(cx));
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

        h_flex()
            .h(TOOLBAR_H)
            .flex_shrink_0()
            .px(px(12.))
            .gap(px(12.))
            .items_center()
            .justify_between()
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
        let nodes = self.visible_entry_nodes();
        let cursor = self.cursor;
        let show_all = self.show_all;
        let revealed = self.revealed.clone();
        let keys_focused = self.zone == Zone::Keys && !self.filter_focused(Zone::Keys, window, cx);
        let editing_node = self.editor.as_ref().and_then(|editor| editor.node);
        let editing_new = self
            .editor
            .as_ref()
            .is_some_and(|editor| matches!(editor.kind, EditKind::New));
        let query_active = !self.key_query.trim().is_empty();
        let entity = cx.entity();

        let rows_src: Vec<(usize, String, String, bool)> = nodes
            .iter()
            .filter_map(|node| {
                let open = self.open.as_ref()?;
                match open.document.nodes.get(*node)? {
                    Node::Entry(entry) => Some((
                        *node,
                        entry.key.clone(),
                        entry.value.clone(),
                        entry.exported,
                    )),
                    Node::Verbatim(_) => None,
                }
            })
            .collect();

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
                    .w(KEY_COL_W)
                    .flex_shrink_0()
                    .pr(px(16.))
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.fg_tertiary)
                    .child("KEY"),
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

        if rows_src.is_empty() && !editing_new && self.open.is_some() {
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

        for (row_ix, (node, key, value, exported)) in rows_src.iter().enumerate() {
            let selected = cursor == row_ix;
            let visible = show_all || revealed.contains(key);
            let click_entity = entity.clone();
            let copy_entity = entity.clone();
            let reveal_entity = entity.clone();
            let key_group = SharedString::from(format!("key-row-{node}"));

            let show_value_input = editing_node == Some(*node)
                && self
                    .editor
                    .as_ref()
                    .is_some_and(|editor| matches!(editor.kind, EditKind::Value));
            let show_rename_input = editing_node == Some(*node)
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
                .mx(px(4.))
                .px(px(7.))
                .h(if editing_this { px(36.) } else { KEY_ROW_H })
                .flex_shrink_0()
                .rounded(px(6.))
                .items_center()
                .overflow_hidden()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    click_entity.update(cx, |this, cx| {
                        this.zone = Zone::Keys;
                        this.cursor = row_ix;
                        if event.click_count >= 2 {
                            this.begin_value_edit(window, cx);
                        } else {
                            this.focus_self(window, cx);
                            cx.notify();
                        }
                    });
                });

            if let Some(bg) = row_bg {
                row = row.bg(bg);
            } else {
                row = row.hover(move |style| style.bg(p.bg_hover));
            }

            let key_color = if query_active || (selected && keys_focused) {
                p.accent
            } else {
                p.fg_primary
            };
            let full_key_tip = if *exported {
                format!("export {key}")
            } else {
                key.clone()
            };

            let key_cell = if show_rename_input {
                if let Some(key_input) = self.editor.as_ref().and_then(|e| e.key.as_ref()) {
                    div()
                        .w(KEY_COL_W)
                        .flex_shrink_0()
                        .pr(px(12.))
                        .font_family(mono.clone())
                        .child(Input::new(key_input).small())
                        .into_any_element()
                } else {
                    div().w(KEY_COL_W).flex_shrink_0().into_any_element()
                }
            } else {
                let mut cell = h_flex()
                    .id(SharedString::from(format!("key-name-{node}")))
                    .w(KEY_COL_W)
                    .flex_shrink_0()
                    .pr(px(16.))
                    .min_w(px(0.))
                    .items_center()
                    .font_family(mono.clone())
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .tooltip(hover_tip(full_key_tip));
                if *exported {
                    cell = cell.child(
                        div()
                            .flex_shrink_0()
                            .text_color(p.fg_tertiary)
                            .child("export "),
                    );
                }
                cell.child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(key_color)
                        .child(key.clone()),
                )
                .into_any_element()
            };

            let value_cell = if show_value_input {
                if let Some(editor) = &self.editor {
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .pr(px(8.))
                        .font_family(mono.clone())
                        .child(
                            Input::new(&editor.value)
                                .small()
                                .on_paste(block_paste(cx.entity().downgrade(), self.focus.clone())),
                        )
                        .into_any_element()
                } else {
                    div().flex_1().into_any_element()
                }
            } else {
                let shown = display_value(value, visible);
                let dim_empty = value.is_empty();
                let value_color = if dim_empty {
                    p.fg_tertiary
                } else if visible {
                    p.fg_primary
                } else {
                    p.fg_secondary
                };
                let value_tip = if visible && !dim_empty {
                    value.clone()
                } else {
                    String::new()
                };
                div()
                    .id(SharedString::from(format!("key-value-{node}")))
                    .flex_1()
                    .min_w(px(0.))
                    .pr(px(8.))
                    .font_family(mono.clone())
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .truncate()
                    .text_color(value_color)
                    .when(!value_tip.is_empty(), |cell| {
                        cell.tooltip(hover_tip(value_tip))
                    })
                    .child(shown)
                    .into_any_element()
            };

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
                actions = actions
                    .child(
                        row_icon_button(
                            SharedString::from(format!("copy-{node}")),
                            IconName::Copy,
                            "Copy value · C",
                        )
                        .text_color(p.fg_secondary)
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
        }

        if editing_new {
            if let Some(editor) = &self.editor {
                let mut row = h_flex()
                    .mx(px(4.))
                    .mt(px(4.))
                    .px(px(7.))
                    .h(px(36.))
                    .flex_shrink_0()
                    .rounded(px(6.))
                    .items_center()
                    .bg(p.bg_selected_accent);
                let paste = block_paste(cx.entity().downgrade(), self.focus.clone());
                if let Some(key_input) = &editor.key {
                    row = row.child(
                        div()
                            .w(KEY_COL_W)
                            .flex_shrink_0()
                            .pr(px(12.))
                            .font_family(mono.clone())
                            .child(Input::new(key_input).small().on_paste(block_paste(
                                cx.entity().downgrade(),
                                self.focus.clone(),
                            ))),
                    );
                }
                row = row
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .pr(px(8.))
                            .font_family(mono.clone())
                            .child(Input::new(&editor.value).small().on_paste(paste)),
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
            }
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
                    .bg(p.danger_bg)
                    .border_b_1()
                    .border_color(p.danger_border)
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                Icon::new(IconName::TriangleAlert)
                                    .size(px(14.))
                                    .text_color(p.danger),
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
                .mx(px(4.))
                .my(px(4.))
                .rounded(px(8.))
                .border_1()
                .border_color(if keys_focused {
                    p.accent_ring
                } else {
                    p.bg_canvas.opacity(0.)
                })
                .overflow_hidden()
                .child(table)
                .child(Scrollbar::vertical(&self.key_scroll).mode(ScrollbarMode::Hover)),
        )
    }

    fn render_footer(&self, p: Palette, mono: SharedString) -> gpui_kit::Div {
        let path_label = self
            .open
            .as_ref()
            .map(|open| open.path.display().to_string())
            .unwrap_or_default();
        let status = self.status.clone();
        let dirty = self.open.as_ref().is_some_and(|open| open.is_dirty());

        let mut left = h_flex().flex_1().min_w(px(0.)).items_center().gap(px(8.));
        if dirty {
            left = left.child(
                div()
                    .id("unsaved-dot")
                    .size(px(6.))
                    .rounded(px(999.))
                    .bg(p.warning_dot)
                    .flex_shrink_0()
                    .tooltip(hover_tip("Unsaved changes · Ctrl+S saves")),
            );
        }
        if !status.is_empty() {
            left = left.child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(11.5))
                    .line_height(px(16.))
                    .text_color(p.fg_secondary)
                    .truncate()
                    .child(status),
            );
        } else if !path_label.is_empty() {
            left = left.child(
                div()
                    .id("footer-path")
                    .flex_1()
                    .min_w(px(0.))
                    .font_family(mono.clone())
                    .text_size(px(11.))
                    .line_height(px(14.))
                    .text_color(p.fg_tertiary)
                    .truncate()
                    .tooltip(hover_tip(path_label.clone()))
                    .child(path_label),
            );
        }

        let hints: &[(&str, &str)] = if self.diff_open {
            &[
                ("↑↓", "Move"),
                ("Space", "Reveal"),
                ("]", "Other file"),
                ("Esc", "Close"),
            ]
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
                    ("Space", "Reveal"),
                    ("C", "Copy"),
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
        let left_name = self.side_name(DiffSide::Left);
        let right_name = self.side_name(DiffSide::Right);
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
                            .items_center()
                            .gap(px(6.))
                            .min_w(px(0.))
                            .child(side_badge("L", p))
                            .child(
                                div()
                                    .font_family(mono.clone())
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.fg_primary)
                                    .truncate()
                                    .child(left_name.clone()),
                            ),
                    )
                    .when(right_only_count > 0, |bar| {
                        bar.child(
                            Button::new("copy-missing-to-left")
                                .outline()
                                .xsmall()
                                .icon(IconName::ArrowLeft)
                                .label("Copy missing")
                                .tooltip(format!(
                                    "Copy {right_only_count} missing into {left_name}"
                                ))
                                .on_click(move |_, _, cx| {
                                    missing_left_entity.update(cx, |this, cx| {
                                        this.copy_missing(DiffSide::Right, cx);
                                    });
                                }),
                        )
                    }),
            )
            .child(div().w(DIFF_GUTTER_W).flex_shrink_0())
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
                            .items_center()
                            .gap(px(6.))
                            .min_w(px(0.))
                            .child(side_badge("R", p))
                            .child(
                                div()
                                    .font_family(mono.clone())
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.fg_primary)
                                    .truncate()
                                    .child(right_name.clone()),
                            ),
                    )
                    .when(left_only_count > 0, |bar| {
                        bar.child(
                            Button::new("copy-missing-to-right")
                                .outline()
                                .xsmall()
                                .icon(IconName::ArrowRight)
                                .label("Copy missing")
                                .tooltip(format!(
                                    "Copy {left_only_count} missing into {right_name}"
                                ))
                                .on_click(move |_, _, cx| {
                                    missing_right_entity.update(cx, |this, cx| {
                                        this.copy_missing(DiffSide::Left, cx);
                                    });
                                }),
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
                    (IconName::Minus, p.danger, Some(p.diff_only_right_bg))
                }
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
            } else if left_empty {
                p.fg_tertiary
            } else if shown {
                p.fg_primary
            } else {
                p.fg_secondary
            };
            let right_color = if row.right.is_none() {
                p.fg_tertiary.opacity(0.6)
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
                .mx(px(4.))
                .px(px(7.))
                .h(DIFF_ROW_H)
                .flex_shrink_0()
                .rounded(px(6.))
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

            if let Some(bg) = tint {
                line = line.bg(bg);
            } else {
                line = line.hover(move |style| style.bg(p.bg_hover.opacity(0.7)));
            }
            if selected {
                line = line.border_1().border_color(p.accent_ring);
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
                        .text_color(if row.kind == diff::DiffKind::Same {
                            p.fg_secondary
                        } else {
                            p.fg_primary
                        })
                        .child(row.key.clone()),
                );

            let left_col = div()
                .flex_1()
                .min_w(px(0.))
                .pr(px(8.))
                .font_family(mono.clone())
                .text_size(px(13.))
                .line_height(px(18.))
                .truncate()
                .text_color(left_color)
                .child(left_text);

            let right_slot = if can_copy_right {
                div()
                    .w(px(24.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        row_icon_button(
                            SharedString::from(format!("diff-to-right-{index}")),
                            IconName::ArrowRight,
                            format!("Copy to {right_name}"),
                        )
                        .text_color(p.fg_secondary)
                        .on_click(move |_, _, cx| {
                            to_right.update(cx, |this, cx| {
                                this.copy_diff_row(index, DiffSide::Left, cx);
                            });
                        }),
                    )
            } else {
                div().w(px(24.)).h(px(24.))
            };

            let left_slot = if can_copy_left {
                div()
                    .w(px(24.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        row_icon_button(
                            SharedString::from(format!("diff-to-left-{index}")),
                            IconName::ArrowLeft,
                            format!("Copy to {left_name}"),
                        )
                        .text_color(p.fg_secondary)
                        .on_click(move |_, _, cx| {
                            to_left.update(cx, |this, cx| {
                                this.copy_diff_row(index, DiffSide::Right, cx);
                            });
                        }),
                    )
            } else {
                div().w(px(24.)).h(px(24.))
            };

            let gutter = h_flex()
                .w(DIFF_GUTTER_W)
                .flex_shrink_0()
                .gap(px(4.))
                .items_center()
                .justify_center()
                .child(right_slot)
                .child(left_slot);

            let right_col = div()
                .flex_1()
                .min_w(px(0.))
                .pl(px(8.))
                .font_family(mono.clone())
                .text_size(px(13.))
                .line_height(px(18.))
                .truncate()
                .text_color(right_color)
                .child(right_text);

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
                .mx(px(4.))
                .my(px(4.))
                .rounded(px(8.))
                .border_1()
                .border_color(if keys_focused {
                    p.accent_ring
                } else {
                    p.bg_canvas.opacity(0.)
                })
                .overflow_hidden()
                .child(body)
                .child(Scrollbar::vertical(&self.diff_scroll).mode(ScrollbarMode::Hover)),
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
                            .child(Input::new(&self.hotkey_input).small()),
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
                    .child(Input::new(&self.folder_input).small())
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
        let pairs: Vec<(String, String)> = report
            .rows
            .into_iter()
            .filter_map(|row| {
                let (kind, value) = match from {
                    DiffSide::Left => (diff::DiffKind::OnlyLeft, row.left),
                    DiffSide::Right => (diff::DiffKind::OnlyRight, row.right),
                };
                if row.kind == kind {
                    Some((row.key, value.unwrap_or_default()))
                } else {
                    None
                }
            })
            .collect();
        let to = match from {
            DiffSide::Left => DiffSide::Right,
            DiffSide::Right => DiffSide::Left,
        };
        self.put_pairs(to, pairs, cx);
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
        let value = match from {
            DiffSide::Left => row.left,
            DiffSide::Right => row.right,
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
        self.put_pairs(to, vec![(row.key, value)], cx);
    }

    fn put_pairs(&mut self, side: DiffSide, pairs: Vec<(String, String)>, cx: &mut Context<Self>) {
        if pairs.is_empty() {
            self.status = "Nothing to copy.".into();
            cx.notify();
            return;
        }
        let count = pairs.len();
        let Some(path) = self.side_path(side) else {
            return;
        };
        let onto_open = self.open.as_ref().is_some_and(|open| open.path == path);
        if onto_open {
            let Some(open) = self.open.as_mut() else {
                return;
            };
            open.mutate(|document| write_pairs(document, &pairs));
            let cloned = self
                .open
                .as_ref()
                .map(|open| open.document.clone())
                .unwrap_or_default();
            match side {
                DiffSide::Left => self.diff_left_doc = Some(cloned),
                DiffSide::Right => self.diff_right_doc = Some(cloned),
            }
            self.mark_edited();
            self.status = format!("Copied {count} into the open file. Ctrl+S saves.");
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
            write_pairs(doc, &pairs);
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
        self.io_queue.push_back(IoJob::Save {
            seq: u64::MAX,
            epoch: self.io_epoch,
            path,
            text,
        });
        self.pump_io(cx);
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
    ("↑  ↓", "Move in this area"),
    ("Enter", "Open the project, or edit the value"),
    ("Tab", "Next area"),
    ("Shift+Tab", "Previous area"),
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
    ("n", "New key"),
    ("Ctrl+S", "Save"),
    ("Ctrl+Z", "Undo"),
    ("Ctrl+O", "Add a folder"),
    ("?", "This list"),
];

enum IoDone {
    Saved {
        seq: u64,
        epoch: u64,
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
    Quit,
}

fn run_io(job: IoJob) -> IoDone {
    match job {
        IoJob::Save {
            seq,
            epoch,
            path,
            text,
        } => {
            let result = save_text(&path, &text);
            IoDone::Saved {
                seq,
                epoch,
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
        IoJob::Quit => IoDone::Quit,
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

fn summary_pill(
    text: String,
    fg: gpui_kit::Hsla,
    bg: gpui_kit::Hsla,
    border: gpui_kit::Hsla,
) -> gpui_kit::Div {
    div()
        .h(px(22.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded(px(999.))
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_size(px(11.))
        .line_height(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .child(text)
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
        btn = btn.bg(p.accent_subtle).text_color(p.accent);
    } else if disabled {
        btn = btn.text_color(p.fg_tertiary.opacity(0.45));
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
