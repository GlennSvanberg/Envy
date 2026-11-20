//! The Envy window.
//!
//! Three zones: projects, the open file, and a diff that stays closed until
//! asked for. Secret values are rendered from memory and are never written to
//! the status line.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use envy_core::discover::{default_file_index, is_example_filename, resolve_file, resolve_project};
use envy_core::document::Node;
use envy_core::paste::{PasteChange, PastePlan};
use envy_core::search::{SearchCommand, SearchHit, SearchKind};
use envy_core::{
    diff, from_example, is_env_key, plan_paste, search, Config, Deeplink, EnvDocument, OpenFile,
    Project, ScanError, ThemeSetting,
};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Selectable, Sizable, Theme, ThemeMode};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, rgb, size, AnyWindowHandle, App, Bounds, ClipboardItem, Context, Entity, FocusHandle,
    Focusable, KeyDownEvent, MouseButton, SharedString, Subscription, Task, TitlebarOptions,
    Window, WindowBounds, WindowOptions,
};

use crate::actions::*;
use crate::hotkey_os::{self, Registration};
use crate::instance::Instance;

const CLEAR_AFTER: Duration = Duration::from_secs(30);
const RESCAN_AFTER: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone {
    Projects,
    Keys,
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
    Palette,
    Settings,
    AddFolder,
    Paste(PastePlan),
    Delete { node: usize, key: String },
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
    open: Option<OpenFile>,
    zone: Zone,
    cursor: usize,
    revealed: HashSet<String>,
    show_all: bool,
    editor: Option<Editor>,
    edit_subs: Vec<Subscription>,
    overlay: Overlay,
    palette_input: Entity<InputState>,
    folder_input: Entity<InputState>,
    hotkey_input: Entity<InputState>,
    input_subs: Vec<Subscription>,
    palette_hits: Vec<SearchHit>,
    palette_ix: usize,
    diff_open: bool,
    diff_ix: Option<usize>,
    diff_doc: Option<EnvDocument>,
    status: String,
    hotkey: Option<Registration>,
    clear_token: u64,
    clear_task: Option<Task<()>>,
    poll_task: Option<Task<()>>,
    scan_rx: Option<Receiver<(Vec<Project>, Vec<ScanError>)>>,
    last_scan: Instant,
}

impl EnvyApp {
    pub fn new(
        instance: Option<Instance>,
        launch: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let config = Config::load();
        let palette_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Projects, files, and keys"));
        let folder_input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder path"));
        let hotkey_input = cx.new(|cx| {
            InputState::new(window, cx).default_value(config.settings.global_hotkey.clone())
        });
        let (projects, scan_errors) = envy_core::discover(&config.folders, &config);
        let focus = cx.focus_handle();
        let mut app = Self {
            focus: focus.clone(),
            window: window.window_handle(),
            instance,
            config,
            projects,
            scan_errors,
            project_ix: None,
            file_ix: None,
            open: None,
            zone: Zone::Projects,
            cursor: 0,
            revealed: HashSet::new(),
            show_all: false,
            editor: None,
            edit_subs: Vec::new(),
            overlay: Overlay::None,
            palette_input,
            folder_input,
            hotkey_input,
            input_subs: Vec::new(),
            palette_hits: Vec::new(),
            palette_ix: 0,
            diff_open: false,
            diff_ix: None,
            diff_doc: None,
            status: String::new(),
            hotkey: None,
            clear_token: 0,
            clear_task: None,
            poll_task: None,
            scan_rx: None,
            last_scan: Instant::now(),
        };
        app.apply_theme(window, cx);
        if let Err(err) = hotkey_os::replace(&mut app.hotkey, &app.config.settings.global_hotkey) {
            app.status = format!("Global hotkey off: {err}");
        }
        app.wire_inputs(window, cx);
        app.poll_task = Some(app.spawn_poll(cx));
        if app.projects.is_empty() {
            app.status = "Add a folder to start. Ctrl+O".into();
        } else {
            app.select_project(0, false, window, cx);
        }
        if let Some(raw) = launch.as_deref().filter(|raw| raw.starts_with("envy:")) {
            match Deeplink::parse(raw) {
                Ok(link) => app.follow_link(link, window, cx),
                Err(err) => app.status = err.to_string(),
            }
        }
        let focus = app.focus.clone();
        cx.defer_in(window, move |this, window, cx| {
            window.focus(&this.focus, cx);
            let _ = focus;
        });
        app
    }

    pub fn window_options(bounds_cx: &mut App) -> WindowOptions {
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), bounds_cx);
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Envy".into()),
                appears_transparent: false,
                traffic_light_position: None,
            }),
            app_id: Some("envy".into()),
            window_min_size: Some(size(px(760.), px(480.))),
            focus: true,
            show: true,
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
            &self.palette_input,
            window,
            |this, _, event, _window, cx| {
                if let InputEvent::Change = event {
                    if matches!(this.overlay, Overlay::Palette) {
                        this.refresh_palette_with(cx);
                        cx.notify();
                    }
                }
            },
        ));
        self.input_subs.push(cx.subscribe_in(
            &self.folder_input,
            window,
            |this, state, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let path = state.read(cx).value().to_string();
                    this.commit_folder(path, window, cx);
                }
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
        let changed = self
            .open
            .as_mut()
            .and_then(|open| open.poll_disk().ok())
            .unwrap_or(false);
        if changed {
            self.status = "Changed on disk. R reloads, Esc keeps yours.".into();
            cx.notify();
        }
        if self.last_scan.elapsed() >= RESCAN_AFTER {
            self.start_scan();
        }
        let finished = match self.scan_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(message)) => Some(message),
            Some(Err(TryRecvError::Empty)) | None => None,
            Some(Err(TryRecvError::Disconnected)) => {
                self.scan_rx = None;
                None
            }
        };
        if let Some((projects, errors)) = finished {
            self.scan_rx = None;
            if !projects.is_empty() || errors.is_empty() {
                self.apply_projects(projects, errors);
            }
            cx.notify();
        }
        if self.config.settings.theme == ThemeSetting::System {
            Theme::sync_system_appearance(None, cx);
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

    fn apply_theme(&self, window: &mut Window, cx: &mut App) {
        match self.config.settings.theme {
            ThemeSetting::System => Theme::sync_system_appearance(Some(window), cx),
            ThemeSetting::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            ThemeSetting::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        }
    }

    fn start_scan(&mut self) {
        if self.scan_rx.is_some() || self.config.folders.is_empty() {
            return;
        }
        self.last_scan = Instant::now();
        let (tx, rx) = mpsc::channel();
        let folders = self.config.folders.clone();
        let config = self.config.clone();
        std::thread::spawn(move || {
            let _ = tx.send(envy_core::discover(&folders, &config));
        });
        self.scan_rx = Some(rx);
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
            .or_else(|| {
                if self.projects.is_empty() {
                    None
                } else {
                    Some(0)
                }
            });
        if let (Some(project_ix), Some(file)) = (self.project_ix, file) {
            self.file_ix = self.projects[project_ix]
                .files
                .iter()
                .position(|item| item.path == file);
        }
    }

    fn commands(&self) -> Vec<SearchCommand> {
        [
            ("add-folder", "Add folder", "Ctrl+O"),
            ("new-key", "New key", "N"),
            (
                "from-example",
                "New .env.local from example",
                "Ctrl+Shift+N",
            ),
            ("diff", "Toggle diff", "D"),
            ("show-all", "Show all values", "A"),
            ("hide-all", "Hide all values", ""),
            ("settings", "Settings", "Ctrl+,"),
            ("refresh", "Rescan folders", "Ctrl+R"),
            ("copy", "Copy value", "C"),
            ("quit", "Quit", ""),
        ]
        .into_iter()
        .map(|(id, label, detail)| SearchCommand {
            id: id.into(),
            label: label.into(),
            detail: detail.into(),
        })
        .collect()
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

    fn selected_node(&self) -> Option<usize> {
        self.entry_nodes().get(self.cursor).copied()
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
        self.project_ix = Some(index);
        self.file_ix = default_file_index(&self.projects[index]);
        if focus_keys {
            self.zone = Zone::Keys;
        }
        self.load_selected_file();
        self.focus_self(window, cx);
        cx.notify();
    }

    fn select_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project_ix.and_then(|ix| self.projects.get(ix)) else {
            return;
        };
        if index >= project.files.len() {
            return;
        }
        self.file_ix = Some(index);
        self.zone = Zone::Keys;
        self.load_selected_file();
        self.focus_self(window, cx);
        cx.notify();
    }

    fn load_selected_file(&mut self) {
        let Some(path) = self.current_path() else {
            self.open = None;
            return;
        };
        if self.open.as_ref().is_some_and(|open| open.path == path) {
            return;
        }
        if let Some(open) = self.open.as_mut() {
            if let Err(err) = open.save() {
                self.status = format!("Save failed: {err}");
                return;
            }
        }
        match OpenFile::load(&path) {
            Ok(open) => {
                self.open = Some(open);
                self.cursor = 0;
                self.revealed.clear();
                self.close_editor();
                self.choose_diff_target();
                self.load_diff();
            }
            Err(err) => {
                self.open = None;
                self.status = format!("Could not open file: {err}");
            }
        }
    }

    fn choose_diff_target(&mut self) {
        let Some(project) = self.project_ix.and_then(|ix| self.projects.get(ix)) else {
            self.diff_ix = None;
            return;
        };
        let current = self.file_ix;
        self.diff_ix = project
            .files
            .iter()
            .enumerate()
            .find(|(index, file)| Some(*index) != current && is_example_filename(&file.name))
            .map(|(index, _)| index)
            .or_else(|| {
                project
                    .files
                    .iter()
                    .enumerate()
                    .find(|(index, _)| Some(*index) != current)
                    .map(|(index, _)| index)
            });
    }

    fn load_diff(&mut self) {
        let Some(path) = self.diff_path() else {
            self.diff_doc = None;
            return;
        };
        self.diff_doc = std::fs::read(&path)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(|text| EnvDocument::parse(&text));
    }

    fn diff_path(&self) -> Option<PathBuf> {
        let project = self.projects.get(self.project_ix?)?;
        Some(project.files.get(self.diff_ix?)?.path.clone())
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

    fn save_open(&mut self) {
        let saved = match self.open.as_mut() {
            Some(open) => open.save(),
            None => return,
        };
        match saved {
            Ok(()) => self.sync_keys(),
            Err(err) => self.status = format!("Save failed: {err}"),
        }
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

    fn follow_link(&mut self, link: Deeplink, window: &mut Window, cx: &mut Context<Self>) {
        self.follow_link_inner(&link);
        self.focus_self(window, cx);
        cx.notify();
    }

    fn follow_link_cx(&mut self, link: Deeplink, cx: &mut Context<Self>) {
        self.follow_link_inner(&link);
        cx.notify();
    }

    fn follow_link_inner(&mut self, link: &Deeplink) {
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
        self.load_selected_file();
        if let Some(key) = &link.key {
            self.jump_to_key(key);
            self.status = format!("Jumped to {key}");
        }
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
            self.zone = Zone::Keys;
        }
    }

    fn move_delta(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        match self.zone {
            Zone::Projects => {
                let len = self.projects.len() as isize;
                if len == 0 {
                    return;
                }
                let current = self.project_ix.unwrap_or(0) as isize;
                let next = (current + delta).clamp(0, len - 1) as usize;
                self.select_project(next, false, window, cx);
            }
            Zone::Keys => {
                let len = self.entry_nodes().len() as isize;
                if len == 0 {
                    return;
                }
                let next = (self.cursor as isize + delta).clamp(0, len - 1) as usize;
                self.cursor = next;
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
            Overlay::Palette => {
                self.activate_palette(window, cx);
                return;
            }
            Overlay::Paste(_) => {
                self.apply_overlay_paste(window, cx);
                return;
            }
            Overlay::Delete { .. } => {
                self.apply_delete(window, cx);
                return;
            }
            Overlay::AddFolder => return,
            Overlay::Settings => return,
            Overlay::None => {}
        }
        if self.zone == Zone::Projects {
            if let Some(index) = self.project_ix {
                self.select_project(index, true, window, cx);
            }
            return;
        }
        self.begin_value_edit(window, cx);
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
        if self.open.as_ref().is_some_and(|open| open.external_change) {
            if let Some(open) = self.open.as_mut() {
                open.dismiss_external();
            }
            self.status = "Keeping your copy.".into();
            cx.notify();
            return;
        }
        if self.diff_open {
            self.diff_open = false;
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
        self.save_open();
        self.status = "Updated.".into();
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
        self.save_open();
        self.jump_to_key(&key);
        self.status = format!("Renamed to {key}.");
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
        self.save_open();
        self.jump_to_key(&key);
        self.status = if exists {
            format!("Updated {key}.")
        } else {
            format!("Added {key}.")
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
        let Some(key) = self.selected_key() else {
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

    fn ask_delete(&mut self, cx: &mut Context<Self>) {
        let Some(node) = self.selected_node() else {
            return;
        };
        let Some(key) = self.selected_key() else {
            return;
        };
        self.overlay = Overlay::Delete { node, key };
        self.status = "Enter deletes the key. Esc cancels.".into();
        cx.notify();
    }

    fn apply_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::Delete { node, key } = &self.overlay else {
            return;
        };
        let node = *node;
        let key = key.clone();
        self.overlay = Overlay::None;
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.mutate(|document| {
            document.remove_entry_at(node);
        });
        self.cursor = self.cursor.saturating_sub(0);
        let len = self.entry_nodes().len();
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor >= len {
            self.cursor = len - 1;
        }
        self.save_open();
        self.status = format!("Deleted {key}. Ctrl+Z undoes.");
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
        self.save_open();
        self.status = "Undone.".into();
        cx.notify();
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if !open.redo() {
            return;
        }
        self.save_open();
        self.status = "Redone.".into();
        cx.notify();
    }

    fn reload_external(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if !open.external_change {
            return;
        }
        if let Err(err) = open.reload() {
            self.status = format!("Reload failed: {err}");
        } else {
            self.cursor = 0;
            self.sync_keys();
            self.status = "Reloaded from disk.".into();
        }
        cx.notify();
    }

    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::Palette;
        self.palette_ix = 0;
        self.palette_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.refresh_palette_with(cx);
        let handle = self.palette_input.focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    fn refresh_palette_with(&mut self, cx: &App) {
        let query = self.palette_input.read(cx).value().to_string();
        self.palette_hits = search(&self.projects, &self.commands(), &query, 40);
        if self.palette_ix >= self.palette_hits.len() {
            self.palette_ix = 0;
        }
    }

    fn move_palette(&mut self, down: bool, cx: &mut Context<Self>) {
        if self.palette_hits.is_empty() {
            return;
        }
        if down {
            self.palette_ix = (self.palette_ix + 1).min(self.palette_hits.len() - 1);
        } else {
            self.palette_ix = self.palette_ix.saturating_sub(1);
        }
        cx.notify();
    }

    fn activate_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_palette_with(cx);
        let Some(hit) = self.palette_hits.get(self.palette_ix).cloned() else {
            return;
        };
        self.overlay = Overlay::None;
        match hit.kind {
            SearchKind::Command => {
                self.run_command(hit.command.as_deref().unwrap_or(""), window, cx)
            }
            SearchKind::Project => {
                if let Some(index) = hit.project {
                    self.select_project(index, true, window, cx);
                }
            }
            SearchKind::File => {
                if let Some(project) = hit.project {
                    self.project_ix = Some(project);
                    if let Some(file) = hit.file {
                        self.select_file(file, window, cx);
                    }
                }
            }
            SearchKind::Key => {
                if let Some(project) = hit.project {
                    self.project_ix = Some(project);
                    if let Some(file) = hit.file {
                        self.file_ix = Some(file);
                        self.load_selected_file();
                    }
                    if let Some(key) = &hit.key {
                        self.jump_to_key(key);
                        self.status = format!("Jumped to {key}");
                    }
                    self.zone = Zone::Keys;
                    self.focus_self(window, cx);
                    cx.notify();
                }
            }
        }
    }

    fn run_command(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            "add-folder" => self.open_add_folder(window, cx),
            "new-key" => self.begin_new_key(window, cx),
            "from-example" => self.new_from_example(cx),
            "diff" => self.toggle_diff(cx),
            "show-all" => {
                self.show_all = true;
                self.status = "Showing all values.".into();
                cx.notify();
            }
            "hide-all" => {
                self.show_all = false;
                self.revealed.clear();
                self.status = "Values hidden.".into();
                cx.notify();
            }
            "settings" => self.open_settings(window, cx),
            "refresh" => {
                self.last_scan = Instant::now() - RESCAN_AFTER;
                self.start_scan();
                self.status = "Scanning.".into();
                cx.notify();
            }
            "copy" => self.copy_selected(cx),
            "quit" => cx.quit(),
            _ => {}
        }
    }

    fn open_add_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::AddFolder;
        self.folder_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        let handle = self.folder_input.focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    fn commit_folder(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let path = PathBuf::from(path.trim());
        if path.as_os_str().is_empty() || !path.is_dir() {
            self.status = "That folder does not exist.".into();
            cx.notify();
            return;
        }
        self.config.add_folder(path);
        if let Err(err) = self.config.save() {
            self.status = format!("Could not save settings: {err}");
        }
        let (projects, errors) = envy_core::discover(&self.config.folders, &self.config);
        self.apply_projects(projects, errors);
        self.overlay = Overlay::None;
        if self.project_ix.is_none() && !self.projects.is_empty() {
            self.select_project(0, true, window, cx);
        }
        self.status = "Folder added.".into();
        self.focus_self(window, cx);
        cx.notify();
    }

    fn remove_folder(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.config.folders.len() {
            return;
        }
        let folder = self.config.folders[index].clone();
        self.config.remove_folder(&folder);
        let _ = self.config.save();
        let (projects, errors) = envy_core::discover(&self.config.folders, &self.config);
        self.apply_projects(projects, errors);
        self.status = "Folder removed.".into();
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
        let _ = self.config.save();
        self.apply_theme(window, cx);
        cx.notify();
    }

    fn toggle_clear_clipboard(&mut self, cx: &mut Context<Self>) {
        self.config.settings.clear_clipboard = !self.config.settings.clear_clipboard;
        let _ = self.config.save();
        self.status = if self.config.settings.clear_clipboard {
            "Clipboard clears 30s after a copy.".into()
        } else {
            "Clipboard is left alone after a copy.".into()
        };
        cx.notify();
    }

    fn commit_hotkey(&mut self, spec: String, window: &mut Window, cx: &mut Context<Self>) {
        let spec = spec.trim().to_string();
        if let Err(err) = hotkey_os::replace(&mut self.hotkey, &spec) {
            self.status = format!("Hotkey not changed: {err}");
            cx.notify();
            return;
        }
        self.config.settings.global_hotkey = spec;
        let _ = self.config.save();
        self.status = format!("Hotkey is {}.", self.config.settings.global_hotkey);
        self.focus_self(window, cx);
        cx.notify();
    }

    fn toggle_diff(&mut self, cx: &mut Context<Self>) {
        self.diff_open = !self.diff_open;
        if self.diff_open {
            if self.diff_ix.is_none() {
                self.choose_diff_target();
            }
            self.load_diff();
        }
        cx.notify();
    }

    fn cycle_diff(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.project_ix.and_then(|ix| self.projects.get(ix)) else {
            return;
        };
        if project.files.len() < 2 {
            self.status = "This project has one env file.".into();
            cx.notify();
            return;
        }
        let current_file = self.file_ix;
        let start = self.diff_ix.unwrap_or(0);
        let mut next = (start + 1) % project.files.len();
        if Some(next) == current_file {
            next = (next + 1) % project.files.len();
        }
        self.diff_ix = Some(next);
        self.diff_open = true;
        self.load_diff();
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
        let dest = example
            .path
            .parent()
            .unwrap_or(project.root.as_path())
            .join(".env.local");
        if dest.exists() {
            self.status = ".env.local already exists.".into();
            cx.notify();
            return;
        }
        let loaded = match OpenFile::load(&example.path) {
            Ok(open) => open,
            Err(err) => {
                self.status = format!("Could not read example: {err}");
                cx.notify();
                return;
            }
        };
        let document = from_example(&loaded.document);
        match OpenFile::create(&dest, document) {
            Ok(_) => {
                let (projects, errors) = envy_core::discover(&self.config.folders, &self.config);
                self.apply_projects(projects, errors);
                if let Some(project_ix) = self.project_ix {
                    if let Some(file_ix) = self
                        .projects
                        .get(project_ix)
                        .and_then(|project| project.files.iter().position(|file| file.path == dest))
                    {
                        self.file_ix = Some(file_ix);
                        self.load_selected_file();
                    }
                }
                self.status = "Created .env.local from the example.".into();
            }
            Err(err) => self.status = format!("Could not create file: {err}"),
        }
        cx.notify();
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
        self.save_open();
        self.status = format!(
            "Pasted {} added, {} overwritten.",
            plan.added(),
            plan.overwritten()
        );
        self.focus_self(window, cx);
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let mods = &event.keystroke.modifiers;
        if mods.control || mods.alt || mods.platform {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => {
                self.cancel(window, cx);
                cx.stop_propagation();
            }
            "up" | "down" if matches!(self.overlay, Overlay::Palette) => {
                self.refresh_palette_with(cx);
                self.move_palette(event.keystroke.key == "down", cx);
                cx.stop_propagation();
            }
            "enter" if self.capture_enter() => {
                self.confirm(window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn capture_enter(&self) -> bool {
        matches!(
            self.overlay,
            Overlay::Palette | Overlay::Paste(_) | Overlay::Delete { .. }
        )
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
    let shown = count.min(64);
    let mut text = "•".repeat(shown);
    if count > shown {
        text.push_str(&format!(" +{}", count - shown));
    }
    text
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
        // Palette query is read here so the list stays in step with the field
        // even if the change event has not landed yet.
        if matches!(self.overlay, Overlay::Palette) {
            self.refresh_palette_with(cx);
        }

        let theme_bg = cx.theme().background;
        let theme_fg = cx.theme().foreground;
        let theme_muted = cx.theme().muted_foreground;
        let theme_border = cx.theme().border;
        let theme_sidebar = cx.theme().sidebar;
        let theme_sidebar_fg = cx.theme().sidebar_foreground;
        let theme_accent = cx.theme().sidebar_accent;
        let theme_danger = cx.theme().danger;
        let mono = cx.theme().mono_font_family.clone();
        let ui_font = cx.theme().font_family.clone();
        let prod = rgb(0xE5484D);

        let project_rows = self.projects.clone();
        let project_ix = self.project_ix;
        let entity = cx.entity();
        let mut sidebar = v_flex()
            .w(px(232.))
            .h_full()
            .bg(theme_sidebar)
            .text_color(theme_sidebar_fg)
            .border_r_1()
            .border_color(theme_border)
            .child(
                h_flex()
                    .px_3()
                    .h(px(40.))
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("Envy"),
                    )
                    .child(
                        Button::new("add-folder")
                            .ghost()
                            .small()
                            .label("Add")
                            .on_click({
                                let entity = entity.clone();
                                move |_, window, cx| {
                                    entity.update(cx, |this, cx| this.open_add_folder(window, cx));
                                }
                            }),
                    ),
            );
        if project_rows.is_empty() {
            sidebar = sidebar.child(
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(theme_muted)
                    .child("No folders yet."),
            );
        }
        for (index, project) in project_rows.iter().enumerate() {
            let selected = project_ix == Some(index);
            let entity = entity.clone();
            let mut row = h_flex()
                .id(SharedString::from(format!("project-{index}")))
                .px_3()
                .py_1()
                .gap_2()
                .items_center()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    entity.update(cx, |this, cx| this.select_project(index, true, window, cx));
                });
            if selected {
                row = row.bg(theme_accent);
            }
            if project.missing_vs_example {
                row = row.child(
                    div()
                        .size(px(6.))
                        .rounded(px(99.))
                        .bg(theme_danger)
                        .flex_shrink_0(),
                );
            } else {
                row = row.child(div().size(px(6.)).flex_shrink_0());
            }
            row = row.child(
                v_flex()
                    .min_w(px(0.))
                    .child(div().text_sm().child(project.name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme_muted)
                            .child(project.branch.clone()),
                    ),
            );
            sidebar = sidebar.child(row);
        }

        let file_tabs = self
            .project_ix
            .and_then(|ix| self.projects.get(ix))
            .map(|project| project.files.clone())
            .unwrap_or_default();
        let file_ix = self.file_ix;
        let mut tabs = h_flex()
            .w_full()
            .flex_shrink_0()
            .border_b_1()
            .border_color(theme_border)
            .id("file-tabs")
            .overflow_x_scroll();
        for (index, file) in file_tabs.iter().enumerate() {
            let selected = file_ix == Some(index);
            let entity = cx.entity();
            let mut tab = v_flex()
                .id(SharedString::from(format!("file-{index}")))
                .text_sm()
                .cursor_pointer()
                .font_family(mono.clone())
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    entity.update(cx, |this, cx| this.select_file(index, window, cx));
                });
            // A prod accent is its own bar. One border color would paint every edge.
            if file.prod {
                tab = tab.child(div().h(px(2.)).w_full().bg(prod));
            } else {
                tab = tab.child(div().h(px(2.)).w_full());
            }
            let mut label = div().px_3().pt_1().pb_2().border_b_2();
            if selected {
                label = label.border_color(cx.theme().primary).text_color(theme_fg);
            } else {
                label = label.border_color(theme_bg).text_color(theme_muted);
            }
            let name = if file.missing_vs_example {
                format!("{} ·", file.name)
            } else {
                file.name.clone()
            };
            tabs = tabs.child(tab.child(label.child(name)));
        }

        let missing = self.missing_names();
        let nodes = self.entry_nodes();
        let cursor = self.cursor;
        let show_all = self.show_all;
        let revealed = self.revealed.clone();
        let editing_node = self.editor.as_ref().and_then(|editor| editor.node);
        let editing_new = self
            .editor
            .as_ref()
            .is_some_and(|editor| matches!(editor.kind, EditKind::New));
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

        let mut table = v_flex().id("key-table").size_full().overflow_y_scroll();
        if self.open.as_ref().is_some_and(|open| open.external_change) {
            table = table.child(
                div()
                    .px_3()
                    .py_2()
                    .bg(theme_danger)
                    .text_color(cx.theme().danger_foreground)
                    .text_sm()
                    .child("This file changed on disk. R reloads it. Esc keeps what you have."),
            );
        }
        if !missing.is_empty() {
            table = table.child(
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(theme_danger)
                    .child(format!("{} missing vs .env.example", missing.len())),
            );
        }
        if rows_src.is_empty() && self.open.is_some() {
            table = table.child(
                div()
                    .p_4()
                    .text_color(theme_muted)
                    .child("No keys. N adds one."),
            );
        }
        for (row_ix, (node, key, value, exported)) in rows_src.iter().enumerate() {
            let selected = cursor == row_ix && self.zone == Zone::Keys;
            let visible = show_all || revealed.contains(key);
            let entity = cx.entity();
            let key_for_click = key.clone();
            let mut row = h_flex()
                .id(SharedString::from(format!("key-{node}")))
                .px_3()
                .h(px(28.))
                .gap_3()
                .items_center()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    entity.update(cx, |this, cx| {
                        this.zone = Zone::Keys;
                        this.cursor = row_ix;
                        if event.click_count >= 2 {
                            this.begin_value_edit(window, cx);
                        } else {
                            cx.notify();
                        }
                    });
                    let _ = key_for_click;
                });
            if selected {
                row = row.bg(theme_accent);
            }
            if missing.iter().any(|missing_key| missing_key == key) {
                row = row.text_color(theme_danger);
            }
            let key_label = if *exported {
                format!("export {key}")
            } else {
                key.clone()
            };
            row = row.child(
                div()
                    .w(px(240.))
                    .font_family(mono.clone())
                    .text_sm()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(key_label),
            );
            let show_input = editing_node == Some(*node)
                && self.editor.as_ref().is_some_and(|editor| {
                    matches!(editor.kind, EditKind::Value | EditKind::Rename)
                });
            if show_input {
                if let Some(editor) = &self.editor {
                    if matches!(editor.kind, EditKind::Rename) {
                        if let Some(key_input) = &editor.key {
                            row = row.child(div().flex_1().child(Input::new(key_input).small()));
                        }
                    } else {
                        row =
                            row.child(div().flex_1().child(
                                Input::new(&editor.value).small().on_paste(block_paste(
                                    cx.entity().downgrade(),
                                    self.focus.clone(),
                                )),
                            ));
                    }
                }
            } else {
                let shown = display_value(value, visible);
                let dim_empty = value.is_empty();
                row = row.child(
                    div()
                        .flex_1()
                        .font_family(mono.clone())
                        .text_sm()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(if dim_empty { theme_muted } else { theme_fg })
                        .child(shown),
                );
            }
            if selected {
                let entity = cx.entity();
                row = row.child(
                    Button::new(SharedString::from(format!("copy-{node}")))
                        .ghost()
                        .xsmall()
                        .label("Copy")
                        .on_click(move |_, _, cx| {
                            entity.update(cx, |this, cx| this.copy_selected(cx));
                        }),
                );
            }
            table = table.child(row);
        }
        if editing_new {
            if let Some(editor) = &self.editor {
                let mut row = h_flex().px_3().h(px(32.)).gap_2().items_center();
                let paste = block_paste(cx.entity().downgrade(), self.focus.clone());
                if let Some(key_input) = &editor.key {
                    row = row.child(
                        div().w(px(240.)).child(
                            Input::new(key_input)
                                .small()
                                .on_paste(block_paste(cx.entity().downgrade(), self.focus.clone())),
                        ),
                    );
                }
                row = row.child(
                    div()
                        .flex_1()
                        .child(Input::new(&editor.value).small().on_paste(paste)),
                );
                table = table.child(row);
            }
        }

        let path_label = self
            .open
            .as_ref()
            .map(|open| open.path.display().to_string())
            .unwrap_or_default();
        let status = self.status.clone();
        let hints = "j k move   enter edit   c copy   space show   n new   ctrl-k search";

        let mut center = v_flex()
            .flex_1()
            .h_full()
            .min_w(px(0.))
            .bg(theme_bg)
            .text_color(theme_fg)
            .child(tabs)
            .child(div().flex_1().min_h(px(0.)).child(table))
            .child(
                h_flex()
                    .h(px(28.))
                    .px_3()
                    .gap_3()
                    .items_center()
                    .border_t_1()
                    .border_color(theme_border)
                    .text_xs()
                    .text_color(theme_muted)
                    .child(div().flex_1().overflow_hidden().whitespace_nowrap().child(
                        if status.is_empty() {
                            path_label
                        } else {
                            status
                        },
                    ))
                    .child(div().child(hints)),
            );

        if self.open.is_none() {
            center = v_flex()
                .flex_1()
                .h_full()
                .items_center()
                .justify_center()
                .bg(theme_bg)
                .text_color(theme_muted)
                .child("Select a project, or add a folder with Ctrl+O.")
                .child(
                    div()
                        .mt_2()
                        .text_xs()
                        .child("Files stay where they are. Envy does not copy secrets."),
                );
        }

        let mut root = h_flex()
            .id("envy-root")
            .key_context("Envy")
            .track_focus(&self.focus)
            .size_full()
            .bg(theme_bg)
            .text_color(theme_fg)
            .font_family(ui_font)
            .overflow_hidden()
            .on_action(cx.listener(|this, _: &MoveDown, window, cx| this.move_delta(1, window, cx)))
            .on_action(cx.listener(|this, _: &MoveUp, window, cx| this.move_delta(-1, window, cx)))
            .on_action(cx.listener(|this, _: &MoveLeft, window, cx| this.move_file(-1, window, cx)))
            .on_action(cx.listener(|this, _: &MoveRight, window, cx| this.move_file(1, window, cx)))
            .on_action(cx.listener(|this, _: &SwitchZone, window, cx| {
                this.zone = match this.zone {
                    Zone::Projects => Zone::Keys,
                    Zone::Keys => Zone::Projects,
                };
                this.focus_self(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.cancel(window, cx)))
            .on_action(cx.listener(|this, _: &CopyValue, _, cx| this.copy_selected(cx)))
            .on_action(cx.listener(|this, _: &ToggleReveal, _, cx| this.toggle_reveal(cx)))
            .on_action(cx.listener(|this, _: &RevealAll, _, cx| this.toggle_show_all(cx)))
            .on_action(cx.listener(|this, _: &NewKey, window, cx| this.begin_new_key(window, cx)))
            .on_action(cx.listener(|this, _: &RenameKey, window, cx| this.begin_rename(window, cx)))
            .on_action(cx.listener(|this, _: &DeleteKey, _, cx| this.ask_delete(cx)))
            .on_action(cx.listener(|this, _: &UndoChange, _, cx| this.undo(cx)))
            .on_action(cx.listener(|this, _: &RedoChange, _, cx| this.redo(cx)))
            .on_action(cx.listener(|this, _: &Save, _, cx| {
                this.save_open();
                this.status = "Saved.".into();
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &OpenPalette, window, cx| this.open_palette(window, cx)),
            )
            .on_action(cx.listener(|this, _: &Refresh, _, cx| {
                this.last_scan = Instant::now() - RESCAN_AFTER;
                this.start_scan();
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

        if self.diff_open {
            root = root.child(self.diff_panel(
                theme_bg,
                theme_fg,
                theme_muted,
                theme_border,
                theme_danger,
                mono,
                cx,
            ));
        }
        if !matches!(self.overlay, Overlay::None) {
            root = root.child(self.overlay_view(window, cx));
        }
        root
    }
}

impl EnvyApp {
    fn diff_panel(
        &self,
        bg: impl Clone + Into<gpui_kit::Hsla>,
        fg: impl Clone + Into<gpui_kit::Hsla>,
        muted: impl Clone + Into<gpui_kit::Hsla>,
        border: impl Clone + Into<gpui_kit::Hsla>,
        danger: impl Clone + Into<gpui_kit::Hsla>,
        mono: SharedString,
        cx: &App,
    ) -> gpui_kit::Div {
        let _ = (bg, fg);
        let name = self
            .diff_ix
            .and_then(|ix| {
                self.project_ix
                    .and_then(|project| self.projects.get(project))
                    .and_then(|project| project.files.get(ix))
                    .map(|file| file.name.clone())
            })
            .unwrap_or_else(|| "nothing".into());
        let mut panel = v_flex()
            .w(px(300.))
            .h_full()
            .border_l_1()
            .border_color(border.clone())
            .bg(cx.theme().sidebar)
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .border_b_1()
                    .border_color(border)
                    .child(format!("Compared with {name}")),
            );
        let Some(current) = self.open.as_ref().map(|open| &open.document) else {
            return panel.child(div().p_3().text_color(muted).child("No file open."));
        };
        let Some(other) = &self.diff_doc else {
            return panel.child(div().p_3().text_color(muted).child("Nothing to compare."));
        };
        let report = diff::diff(current, other);
        panel = panel.child(
            div()
                .px_3()
                .py_1()
                .text_xs()
                .text_color(muted.clone())
                .child("] cycles files"),
        );
        let mut body = v_flex()
            .id("diff-body")
            .flex_1()
            .overflow_y_scroll()
            .text_sm();
        for row in &report.rows {
            if row.kind == diff::DiffKind::Same {
                continue;
            }
            let mark = match row.kind {
                diff::DiffKind::Different => "≠",
                diff::DiffKind::OnlyLeft => "+",
                diff::DiffKind::OnlyRight => "missing",
                diff::DiffKind::Same => "",
            };
            let color_danger = matches!(
                row.kind,
                diff::DiffKind::OnlyRight | diff::DiffKind::Different
            );
            let left = row.left.as_deref().unwrap_or("");
            let right = row.right.as_deref().unwrap_or("");
            let visible = self.visible(&row.key);
            let detail = match row.kind {
                diff::DiffKind::Different => format!(
                    "{}  vs  {}",
                    display_value(left, visible),
                    display_value(right, visible)
                ),
                diff::DiffKind::OnlyLeft => display_value(left, visible),
                diff::DiffKind::OnlyRight => display_value(right, visible),
                diff::DiffKind::Same => String::new(),
            };
            let mut line = v_flex().px_3().py_1().font_family(mono.clone());
            if color_danger {
                line = line.text_color(danger.clone());
            }
            line = line
                .child(format!("{mark}  {}", row.key))
                .child(div().text_xs().text_color(muted.clone()).child(detail));
            body = body.child(line);
        }
        if !report.has_warnings() && report.only_left().next().is_none() {
            body = body.child(div().p_3().text_color(muted).child("No differences."));
        }
        panel.child(body)
    }

    fn overlay_view(&self, _window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::Div {
        let entity = cx.entity();
        let mut backdrop = div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(cx.theme().background.opacity(0.72))
            .on_mouse_down(MouseButton::Left, {
                let entity = entity.clone();
                move |_, window, cx| {
                    entity.update(cx, |this, cx| this.close_overlay(window, cx));
                }
            });
        let panel = match &self.overlay {
            Overlay::Palette => self.palette_panel(cx),
            Overlay::Settings => self.settings_panel(cx),
            Overlay::AddFolder => self.folder_panel(cx),
            Overlay::Paste(plan) => self.paste_panel(plan, cx),
            Overlay::Delete { key, .. } => self.delete_panel(key, cx),
            Overlay::None => div(),
        };
        backdrop = backdrop.child(
            div()
                .absolute()
                .top(px(64.))
                .left(px(280.))
                .w(px(520.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(panel),
        );
        backdrop
    }

    fn palette_panel(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let mut list = v_flex()
            .id("palette-list")
            .max_h(px(360.))
            .overflow_y_scroll();
        for (index, hit) in self.palette_hits.iter().enumerate() {
            let selected = index == self.palette_ix;
            let mut row = h_flex().px_3().py_1().gap_3().justify_between().text_sm();
            if selected {
                row = row.bg(cx.theme().sidebar_accent);
            }
            row = row
                .child(
                    div()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(hit.label.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(hit.detail.clone()),
                );
            list = list.child(row);
        }
        if self.palette_hits.is_empty() {
            list = list.child(
                div()
                    .p_3()
                    .text_color(cx.theme().muted_foreground)
                    .child("No matches."),
            );
        }
        v_flex()
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .overflow_hidden()
            .child(div().p_2().child(Input::new(&self.palette_input)))
            .child(list)
    }

    fn settings_panel(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let entity = cx.entity();
        let theme = self.config.settings.theme;
        let clear = self.config.settings.clear_clipboard;
        let mut folders = v_flex().gap_1();
        for (index, folder) in self.config.folders.iter().enumerate() {
            let entity = entity.clone();
            folders = folders.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(folder.display().to_string()),
                    )
                    .child(
                        Button::new(SharedString::from(format!("rm-folder-{index}")))
                            .ghost()
                            .xsmall()
                            .label("Remove")
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| this.remove_folder(index, cx));
                            }),
                    ),
            );
        }
        v_flex()
            .p_4()
            .gap_3()
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(
                div()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("Settings"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Theme"),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(theme_button(
                        "theme-system",
                        "System",
                        theme == ThemeSetting::System,
                        ThemeSetting::System,
                        cx,
                    ))
                    .child(theme_button(
                        "theme-light",
                        "Light",
                        theme == ThemeSetting::Light,
                        ThemeSetting::Light,
                        cx,
                    ))
                    .child(theme_button(
                        "theme-dark",
                        "Dark",
                        theme == ThemeSetting::Dark,
                        ThemeSetting::Dark,
                        cx,
                    )),
            )
            .child(
                Button::new("clear-clip")
                    .small()
                    .label(if clear {
                        "Clipboard clears after 30s"
                    } else {
                        "Clipboard is kept"
                    })
                    .selected(clear)
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| this.toggle_clear_clipboard(cx));
                        }
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Global hotkey"),
            )
            .child(Input::new(&self.hotkey_input).small())
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Folders"),
            )
            .child(folders)
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Settings live in the envy config file. Values never do."),
            )
    }

    fn folder_panel(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut panel = v_flex()
            .p_4()
            .gap_2()
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(
                div()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("Add folder"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Path to a directory. Envy looks for .env files inside it."),
            )
            .child(Input::new(&self.folder_input));
        #[cfg(windows)]
        {
            let entity = cx.entity();
            panel = panel.child(
                Button::new("browse-folder")
                    .small()
                    .label("Browse")
                    .on_click(move |_, window, cx| {
                        if let Some(path) =
                            rfd::FileDialog::new().set_title("Add folder").pick_folder()
                        {
                            entity.update(cx, |this, cx| {
                                this.commit_folder(path.display().to_string(), window, cx);
                            });
                        }
                    }),
            );
        }
        let _ = cx;
        panel
    }

    fn paste_panel(&self, plan: &PastePlan, cx: &App) -> gpui_kit::Div {
        let mut list = v_flex()
            .id("folder-list")
            .max_h(px(280.))
            .overflow_y_scroll()
            .gap_1();
        for change in &plan.changes {
            let (mark, key, value) = match change {
                PasteChange::Add { key, value } => ("add", key, value),
                PasteChange::Overwrite { key, value, .. } => ("overwrite", key, value),
            };
            list = list.child(
                h_flex()
                    .justify_between()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_sm()
                    .child(format!("{mark}  {key}"))
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(display_value(value, self.visible(key))),
                    ),
            );
        }
        v_flex()
            .p_4()
            .gap_2()
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(div().font_weight(gpui_kit::FontWeight::SEMIBOLD).child(format!(
                "Paste {} keys",
                plan.changes.len()
            )))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Enter confirms. Esc cancels. Existing lines you do not touch stay byte-for-byte."),
            )
            .child(list)
    }

    fn delete_panel(&self, key: &str, cx: &App) -> gpui_kit::Div {
        v_flex()
            .p_4()
            .gap_2()
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .child(
                div()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(format!("Delete {key}?")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Enter deletes it from the file. Ctrl+Z puts it back."),
            )
    }
}

fn theme_button(
    id: &'static str,
    label: &'static str,
    selected: bool,
    theme: ThemeSetting,
    cx: &mut Context<EnvyApp>,
) -> gpui_kit::component::button::Button {
    let entity = cx.entity();
    let mut button = Button::new(id).small().label(label).selected(selected);
    button = button.on_click(move |_, window, cx| {
        entity.update(cx, |this, cx| this.set_theme(theme, window, cx));
    });
    button
}
