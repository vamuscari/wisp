use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use thiserror::Error;
use wisp_core::{
    config::{Openers, VcsIcons},
    model::{DirectoryEntry, Project},
    navigation::{NavigationError, NavigationOutcome, Navigator, Screen},
    opencode::{OpenCodeSession, OpenCodeSnapshot, SessionDisplayState},
    path::comparison_key,
    protocol::{
        FileHostTarget, FileOpenTarget, FilePreviewState, HostContext, HostWorkspaceContext,
        NvimView, Selection,
    },
};

const THEME: Theme = Theme {
    accent: Color::Cyan,
    active: Color::Green,
    error: Color::Red,
    input: Color::Yellow,
    muted: Color::DarkGray,
};

#[derive(Clone, Copy)]
struct Theme {
    accent: Color,
    active: Color,
    error: Color,
    input: Color,
    muted: Color,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Projects,
    Detail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightMode {
    Windows,
    Files,
    Sessions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuxiliaryPane {
    Hidden,
    Preview,
    Commands,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialView {
    Projects,
    Windows,
    Sessions,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SinglePaneBehavior {
    #[default]
    Show,
    Activate,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitSummary {
    pub branch: String,
    pub dirty: bool,
    pub untracked: usize,
    pub modified: usize,
    pub staged: usize,
    pub conflicted: usize,
    pub ahead: usize,
    pub behind: usize,
    pub stashed: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowPreviewRequest {
    pub request_id: u64,
    pub pane_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowPreviewContent {
    Text(String),
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowPreviewUpdate {
    pub request_id: u64,
    pub pane_id: String,
    pub content: WindowPreviewContent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryRequest {
    pub request_id: u64,
    pub path: PathBuf,
    pub refresh: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryUpdate {
    pub request_id: u64,
    pub path: PathBuf,
    pub result: Result<Vec<DirectoryEntry>, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowPreviewState {
    Disabled,
    Loading,
    Ready(String),
    NoOutput,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveProjectContext {
    pub project_id: String,
    pub file: Option<String>,
    pub git: Option<GitSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    None,
    LoadDirectory(PathBuf),
    LoadSessions(PathBuf),
    RefreshProjects,
    RefreshDirectory(PathBuf),
    RefreshSessions(PathBuf),
    Finish(Selection),
    Cancel,
}

pub trait DataSource {
    fn project_git_updates(&mut self) -> Vec<(String, GitSummary)> {
        Vec::new()
    }
    fn directory(&mut self, path: &Path) -> Result<Vec<DirectoryEntry>, String>;
    fn refresh_projects(&mut self) -> Result<Vec<Project>, String>;
    fn refresh_directory(&mut self, path: &Path) -> Result<Vec<DirectoryEntry>, String>;
    fn request_directory(&mut self, request: DirectoryRequest) -> Option<DirectoryUpdate> {
        let result = if request.refresh {
            self.refresh_directory(&request.path)
        } else {
            self.directory(&request.path)
        };
        Some(DirectoryUpdate {
            request_id: request.request_id,
            path: request.path,
            result,
        })
    }
    fn directory_update(&mut self) -> Option<DirectoryUpdate> {
        None
    }
    fn sessions(&mut self, _path: &Path) -> Result<OpenCodeSnapshot, String> {
        Err("OpenCode integration is not configured".into())
    }
    fn refresh_sessions(&mut self, path: &Path) -> Result<OpenCodeSnapshot, String> {
        self.sessions(path)
    }
    fn session_updates_pending(&mut self) -> bool {
        false
    }
    fn request_window_preview(&mut self, _request: WindowPreviewRequest) {}
    fn cancel_window_preview(&mut self) {}
    fn window_preview_update(&mut self) -> Option<WindowPreviewUpdate> {
        None
    }
    fn publish_file_preview(&mut self, _state: FilePreviewState) -> Result<(), String> {
        Ok(())
    }
}

pub trait Input {
    fn read_key(&mut self) -> io::Result<KeyEvent>;
    fn set_mouse_capture(&mut self, _enabled: bool) -> io::Result<()> {
        Ok(())
    }
    fn read_key_timeout(&mut self, _timeout: Duration) -> io::Result<Option<KeyEvent>> {
        self.read_key().map(Some)
    }
    fn read_event_timeout(&mut self, timeout: Duration) -> io::Result<Option<InputEvent>> {
        self.read_key_timeout(timeout)
            .map(|event| event.map(InputEvent::Key))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize,
}

#[derive(Debug, Error)]
pub enum TuiError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Navigation(#[from] NavigationError),
}

#[derive(Clone, Debug)]
pub struct App {
    navigator: Navigator,
    openers: Openers,
    file_open_target: FileOpenTarget,
    file_preview_available: bool,
    file_preview_visible: bool,
    follow_symlinks: bool,
    context: HostContext,
    file_columns: Vec<FileColumn>,
    file_focus: usize,
    pending_file_load: Option<(usize, PathBuf)>,
    pending_file_focus: Option<usize>,
    queued_file_preview: Option<PathBuf>,
    active_directory_request: Option<u64>,
    opencode_command: Option<Vec<String>>,
    sessions: Vec<OpenCodeSession>,
    session_host_items: BTreeMap<String, String>,
    session_conflicts: BTreeSet<String>,
    project_query: String,
    detail_query: String,
    pane_query: String,
    project_cursor: usize,
    detail_cursor: usize,
    pane_cursor: usize,
    pane_focus: bool,
    single_pane_behavior: SinglePaneBehavior,
    focus: Focus,
    right_mode: RightMode,
    input_mode: InputMode,
    status: Option<String>,
    startup_command: Option<Command>,
    active_project: Option<ActiveProjectContext>,
    project_git: BTreeMap<String, GitSummary>,
    vcs_icons: VcsIcons,
    window_preview_available: bool,
    auxiliary_pane: AuxiliaryPane,
    auxiliary_before_commands: AuxiliaryPane,
    window_preview_request: Option<(u64, String)>,
    window_preview_state: WindowPreviewState,
    window_preview_refresh: u64,
    hovered_detail: Option<usize>,
}

impl App {
    pub fn new(
        projects: Vec<Project>,
        openers: Openers,
        follow_symlinks: bool,
        context: Option<HostContext>,
        initial_view: InitialView,
    ) -> Self {
        Self::build(
            projects,
            openers,
            follow_symlinks,
            context,
            initial_view,
            None,
        )
    }

    pub fn new_with_opencode(
        projects: Vec<Project>,
        openers: Openers,
        follow_symlinks: bool,
        context: Option<HostContext>,
        initial_view: InitialView,
        command: Vec<String>,
    ) -> Self {
        Self::build(
            projects,
            openers,
            follow_symlinks,
            context,
            initial_view,
            Some(command),
        )
    }

    fn build(
        projects: Vec<Project>,
        openers: Openers,
        follow_symlinks: bool,
        context: Option<HostContext>,
        initial_view: InitialView,
        opencode_command: Option<Vec<String>>,
    ) -> Self {
        let mut app = Self {
            navigator: Navigator::new(projects, follow_symlinks),
            openers,
            file_open_target: FileOpenTarget::Window,
            file_preview_available: false,
            file_preview_visible: false,
            follow_symlinks,
            context: context.unwrap_or_default(),
            file_columns: Vec::new(),
            file_focus: 0,
            pending_file_load: None,
            pending_file_focus: None,
            queued_file_preview: None,
            active_directory_request: None,
            opencode_command,
            sessions: Vec::new(),
            session_host_items: BTreeMap::new(),
            session_conflicts: BTreeSet::new(),
            project_query: String::new(),
            detail_query: String::new(),
            pane_query: String::new(),
            project_cursor: 0,
            detail_cursor: 0,
            pane_cursor: 0,
            pane_focus: false,
            single_pane_behavior: SinglePaneBehavior::Show,
            focus: Focus::Projects,
            right_mode: RightMode::Windows,
            input_mode: InputMode::Normal,
            status: None,
            startup_command: None,
            active_project: None,
            project_git: BTreeMap::new(),
            vcs_icons: VcsIcons::default(),
            window_preview_available: false,
            auxiliary_pane: AuxiliaryPane::Hidden,
            auxiliary_before_commands: AuxiliaryPane::Hidden,
            window_preview_request: None,
            window_preview_state: WindowPreviewState::Disabled,
            window_preview_refresh: 0,
            hovered_detail: None,
        };
        app.select_current_project();
        match initial_view {
            InitialView::Projects => {}
            InitialView::Windows => {
                if app.selected_project_is_current() {
                    app.focus = Focus::Detail;
                    app.select_active_host_item();
                } else {
                    app.status = Some("Current workspace is not a Wisp project".into());
                }
            }
            InitialView::Sessions => {
                if app.opencode_command.is_none() {
                    app.status = Some("OpenCode integration is not configured".into());
                } else if app.selected_workspace_name().is_some() {
                    app.set_workspace_project_status();
                } else {
                    app.focus = Focus::Detail;
                    app.right_mode = RightMode::Sessions;
                    app.startup_command = app.selected_project_path().map(Command::LoadSessions);
                }
            }
        }
        app
    }

    pub const fn focus(&self) -> Focus {
        self.focus
    }

    pub const fn right_mode(&self) -> RightMode {
        self.right_mode
    }

    pub const fn input_mode(&self) -> InputMode {
        self.input_mode
    }

    pub fn detail_cursor(&self) -> usize {
        if self.right_mode == RightMode::Files {
            match self.file_columns.get(self.file_focus) {
                Some(column) => column.cursor,
                None => 0,
            }
        } else {
            self.detail_cursor
        }
    }

    pub fn screen(&self) -> Screen {
        self.navigator.screen().clone()
    }

    pub fn selected_project_id(&self) -> Option<&str> {
        let project_id = match self.selected_target()? {
            ProjectTarget::Project(project) => project.id,
            ProjectTarget::Workspace { .. } => return None,
        };
        self.navigator
            .projects()
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.id.as_str())
    }

    pub fn project_query(&self) -> &str {
        &self.project_query
    }

    pub fn detail_query(&self) -> &str {
        if self.right_mode == RightMode::Files {
            self.file_columns
                .get(self.file_focus)
                .map(|column| column.query.as_str())
                .unwrap_or_default()
        } else if self.pane_focus {
            &self.pane_query
        } else {
            &self.detail_query
        }
    }

    fn file_breadcrumb(&self) -> String {
        let parts = self
            .file_columns
            .iter()
            .take(self.file_focus.saturating_add(1))
            .map(|column| {
                column
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| column.path.display().to_string())
            })
            .collect::<Vec<_>>();
        if parts.is_empty() {
            "Files".into()
        } else {
            parts.join(" > ")
        }
    }

    pub fn configure_single_pane_behavior(&mut self, behavior: SinglePaneBehavior) {
        self.single_pane_behavior = behavior;
    }

    pub fn configure_file_open_target(&mut self, target: FileOpenTarget) {
        self.file_open_target = target;
    }

    pub fn configure_file_preview(&mut self, available: bool, initially_visible: bool) {
        self.file_preview_available = available;
        self.file_preview_visible = available && initially_visible;
    }

    pub fn file_preview_state(&self) -> FilePreviewState {
        if self.right_mode != RightMode::Files
            || !self.file_preview_available
            || !self.file_preview_visible
        {
            return FilePreviewState::Hidden;
        }
        let Some((project, entry)) = self.selected_regular_file() else {
            return FilePreviewState::Empty;
        };
        FilePreviewState::File {
            project,
            path: entry.path.clone(),
            nvim_view: self
                .best_nvim_target(&entry.path)
                .map(|(_, nvim_view)| Box::new(nvim_view)),
        }
    }

    pub fn current_directory(&self) -> Option<&Path> {
        let column_path = (self.focus == Focus::Detail && self.right_mode == RightMode::Files)
            .then(|| self.file_columns.get(self.file_focus))
            .flatten()
            .map(|column| column.path.as_path());
        column_path.or_else(|| match self.navigator.screen() {
            Screen::Directory { path, .. } => Some(path.as_path()),
            Screen::Projects => None,
        })
    }

    pub fn visible_project_labels(&self) -> Vec<String> {
        self.visible_project_items()
            .into_iter()
            .map(|item| item.target.display_name().to_string())
            .collect()
    }

    pub fn visible_detail_labels(&self) -> Vec<String> {
        self.visible_detail_items()
            .into_iter()
            .map(|item| item.label)
            .collect()
    }

    pub fn set_active_project_context(&mut self, context: ActiveProjectContext) {
        let update_startup_sessions =
            matches!(&self.startup_command, Some(Command::LoadSessions(_)));
        if let Some(git) = context.git.clone() {
            self.project_git.insert(context.project_id.clone(), git);
        }
        self.active_project = Some(context);
        self.select_current_project();
        if update_startup_sessions {
            self.startup_command = self.selected_project_path().map(Command::LoadSessions);
        }
    }

    pub fn set_active_project_git(&mut self, project_id: &str, git: GitSummary) {
        self.project_git.insert(project_id.to_string(), git);
    }

    pub fn set_vcs_icons(&mut self, icons: VcsIcons) {
        self.vcs_icons = icons;
    }

    pub fn configure_window_preview(&mut self, available: bool, initially_visible: bool) {
        self.window_preview_available = available;
        self.auxiliary_pane = if available && initially_visible {
            AuxiliaryPane::Preview
        } else {
            AuxiliaryPane::Hidden
        };
        self.auxiliary_before_commands = self.auxiliary_pane;
        self.window_preview_request = None;
        self.window_preview_state = if self.window_preview_visible() {
            WindowPreviewState::Unavailable
        } else {
            WindowPreviewState::Disabled
        };
    }

    pub const fn auxiliary_pane(&self) -> AuxiliaryPane {
        self.auxiliary_pane
    }

    pub fn window_preview_visible(&self) -> bool {
        self.window_preview_available
            && self.auxiliary_pane == AuxiliaryPane::Preview
            && self.right_mode == RightMode::Windows
    }

    pub fn window_preview_target(&self) -> Option<String> {
        if !self.window_preview_visible() {
            return None;
        }
        if self.pane_focus {
            self.visible_pane_items()
                .get(self.pane_cursor)
                .and_then(|item| item.pane_id.clone())
        } else {
            self.visible_detail_items()
                .get(self.hovered_detail.unwrap_or(self.detail_cursor))
                .and_then(|item| item.pane_id.clone())
        }
    }

    fn restore_window_preview_target(&mut self, pane_id: &str, prefer_pane: bool) -> bool {
        let original_project_cursor = self.project_cursor;
        let original_detail_cursor = self.detail_cursor;
        let original_pane_cursor = self.pane_cursor;
        let original_pane_focus = self.pane_focus;
        for project_cursor in 0..self.visible_project_items().len() {
            self.project_cursor = project_cursor;
            let window_cursor = self
                .visible_detail_items()
                .iter()
                .position(|item| item.pane_id.as_deref() == Some(pane_id));
            if !prefer_pane {
                if let Some(detail_cursor) = window_cursor {
                    self.detail_cursor = detail_cursor;
                    self.pane_focus = false;
                    return true;
                }
            }
            for detail_cursor in 0..self.visible_detail_items().len() {
                self.detail_cursor = detail_cursor;
                if let Some(pane_cursor) = self
                    .pane_items()
                    .iter()
                    .position(|item| item.pane_id.as_deref() == Some(pane_id))
                {
                    self.pane_query.clear();
                    self.pane_cursor = pane_cursor;
                    self.pane_focus = true;
                    return true;
                }
            }
            if let Some(detail_cursor) = window_cursor {
                self.detail_cursor = detail_cursor;
                self.pane_focus = false;
                return true;
            }
        }
        self.project_cursor = original_project_cursor;
        self.detail_cursor = original_detail_cursor;
        self.pane_cursor = original_pane_cursor;
        self.pane_focus = original_pane_focus;
        false
    }

    pub fn window_preview_state(&self) -> &WindowPreviewState {
        &self.window_preview_state
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        if mouse.kind != MouseEventKind::Moved || !self.window_preview_visible() {
            return;
        }
        let layout = ui_areas(area, true);
        let Some(detail) = layout.detail else {
            self.hovered_detail = None;
            return;
        };
        let (Some(windows), _) = window_pane_areas(detail, self.pane_focus) else {
            self.hovered_detail = None;
            return;
        };
        let inner = inset(windows, 1);
        let inside = mouse.column >= inner.x
            && mouse.column < inner.x.saturating_add(inner.width)
            && mouse.row >= inner.y
            && mouse.row < inner.y.saturating_add(inner.height);
        if !inside {
            self.hovered_detail = None;
            return;
        }
        let items = self.visible_detail_items();
        let offset = list_offset(self.detail_cursor, items.len(), inner.height as usize);
        let index = offset.saturating_add(mouse.row.saturating_sub(inner.y) as usize);
        self.hovered_detail = (index < items.len()).then_some(index);
    }

    fn begin_window_preview(&mut self, request: &WindowPreviewRequest) {
        self.window_preview_request = Some((request.request_id, request.pane_id.clone()));
        self.window_preview_state = WindowPreviewState::Loading;
    }

    fn apply_window_preview(&mut self, update: WindowPreviewUpdate) {
        if self.window_preview_request.as_ref()
            != Some(&(update.request_id, update.pane_id.clone()))
        {
            return;
        }
        self.window_preview_state = match update.content {
            WindowPreviewContent::Text(text) if text.is_empty() => WindowPreviewState::NoOutput,
            WindowPreviewContent::Text(text) => WindowPreviewState::Ready(text),
            WindowPreviewContent::Unavailable => WindowPreviewState::Unavailable,
        };
    }

    fn clear_window_preview(&mut self) {
        self.window_preview_request = None;
        if self.window_preview_visible() {
            self.window_preview_state = WindowPreviewState::Unavailable;
        } else {
            self.window_preview_state = WindowPreviewState::Disabled;
        }
    }

    fn toggle_window_preview(&mut self) {
        if !self.window_preview_available {
            self.status = Some("Window preview is unavailable".into());
            return;
        }
        self.auxiliary_pane = if self.auxiliary_pane == AuxiliaryPane::Preview {
            AuxiliaryPane::Hidden
        } else {
            AuxiliaryPane::Preview
        };
        self.auxiliary_before_commands = self.auxiliary_pane;
        self.clear_window_preview();
    }

    fn toggle_commands(&mut self) {
        if self.auxiliary_pane == AuxiliaryPane::Commands {
            self.auxiliary_pane = self.auxiliary_before_commands;
        } else {
            self.auxiliary_before_commands = self.auxiliary_pane;
            self.auxiliary_pane = AuxiliaryPane::Commands;
        }
        self.clear_window_preview();
    }

    pub fn replace_projects(&mut self, projects: Vec<Project>) {
        self.navigator = Navigator::new(projects, self.follow_symlinks);
        self.project_git.clear();
        self.file_columns.clear();
        self.file_focus = 0;
        self.pending_file_load = None;
        self.pending_file_focus = None;
        self.queued_file_preview = None;
        self.active_directory_request = None;
        self.sessions.clear();
        self.session_host_items.clear();
        self.session_conflicts.clear();
        self.reset_queries_and_cursors();
        self.select_current_project();
    }

    pub fn load_directory(&mut self, mut entries: Vec<DirectoryEntry>) {
        entries.sort_by(|left, right| {
            left.name()
                .to_lowercase()
                .cmp(&right.name().to_lowercase())
                .then_with(|| left.path.cmp(&right.path))
        });
        let (depth, path) = self.pending_file_load.take().unwrap_or_else(|| {
            let path = match self.navigator.screen() {
                Screen::Directory { path, .. } => path.clone(),
                Screen::Projects => PathBuf::new(),
            };
            (self.file_focus, path)
        });
        self.file_columns.truncate(depth);
        self.file_columns.push(FileColumn {
            path,
            entries,
            cursor: 0,
            query: String::new(),
        });
        if self.pending_file_focus == Some(depth) {
            self.file_focus = depth;
            self.pending_file_focus = None;
        } else {
            self.file_focus = self.file_focus.min(depth);
        }
        self.detail_query.clear();
        self.detail_cursor = 0;
        self.pane_query.clear();
        self.pane_cursor = 0;
        self.pane_focus = false;
        self.status = None;
        if self.file_focus == depth {
            self.queued_file_preview = match self.preview_selected_file() {
                Command::LoadDirectory(path) => Some(path),
                _ => None,
            };
        }
    }

    fn take_file_preview_request(&mut self) -> Option<PathBuf> {
        self.queued_file_preview.take()
    }

    fn begin_directory_request(&mut self, request: &DirectoryRequest) {
        if !self
            .pending_file_load
            .as_ref()
            .is_some_and(|(_, path)| path == &request.path)
        {
            self.cancel_pending_directory_request();
            self.pending_file_load = Some((self.file_focus, request.path.clone()));
        }
        self.active_directory_request = Some(request.request_id);
    }

    fn apply_directory_update(&mut self, update: DirectoryUpdate) {
        if self.active_directory_request != Some(update.request_id) {
            return;
        }
        self.active_directory_request = None;
        match update.result {
            Ok(entries) => self.load_directory(entries),
            Err(error) => self.fail_directory_load(error),
        }
    }

    fn fail_directory_load(&mut self, error: impl Into<String>) {
        if self.pending_file_focus.is_some() {
            let _ = self.navigator.back();
        }
        self.pending_file_load = None;
        self.pending_file_focus = None;
        self.queued_file_preview = None;
        self.active_directory_request = None;
        self.set_status(error);
    }

    fn cancel_pending_directory_request(&mut self) -> bool {
        let cancelled_descent = self.pending_file_focus.take().is_some();
        if cancelled_descent {
            let _ = self.navigator.back();
        }
        self.pending_file_load = None;
        self.queued_file_preview = None;
        self.active_directory_request = None;
        cancelled_descent
    }

    pub fn load_sessions(&mut self, snapshot: OpenCodeSnapshot) {
        let selected_session_id = (self.right_mode == RightMode::Sessions)
            .then(|| self.visible_detail_items().get(self.detail_cursor).cloned())
            .flatten()
            .and_then(|item| match item.target {
                DetailTarget::OpenCodeSession(id) => Some(id),
                DetailTarget::HostWindow(_)
                | DetailTarget::HostPane { .. }
                | DetailTarget::Entry(_) => None,
            });
        self.sessions = snapshot.sessions;
        self.session_host_items = snapshot.host_items;
        self.session_conflicts = snapshot.conflicts;
        let visible = self.visible_detail_items();
        self.detail_cursor = selected_session_id
            .and_then(|selected| {
                visible.iter().position(|item| {
                    matches!(&item.target, DetailTarget::OpenCodeSession(id) if id == &selected)
                })
            })
            .unwrap_or_else(|| self.detail_cursor.min(visible.len().saturating_sub(1)));
        self.status = None;
    }

    fn take_startup_command(&mut self) -> Option<Command> {
        self.startup_command.take()
    }

    fn selected_project_path(&self) -> Option<PathBuf> {
        let project_id = self.selected_project_id()?;
        self.navigator
            .projects()
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
    }

    fn selected_target(&self) -> Option<ProjectTarget> {
        self.visible_project_items()
            .get(self.project_cursor)
            .map(|item| item.target.clone())
    }

    fn selected_target_key(&self) -> Option<ProjectTargetKey> {
        self.selected_target().map(|target| target.key())
    }

    fn selected_workspace_name(&self) -> Option<String> {
        match self.selected_target()? {
            ProjectTarget::Project(_) => None,
            ProjectTarget::Workspace { name, .. } => Some(name),
        }
    }

    fn set_workspace_project_status(&mut self) {
        if let Some(workspace) = self.selected_workspace_name() {
            self.status = Some(format!("Workspace {workspace} is not a Wisp project"));
        }
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = Some(status.into());
    }

    pub fn clear_status(&mut self) {
        self.status = None;
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<Command, NavigationError> {
        if key.kind == KeyEventKind::Release {
            return Ok(Command::None);
        }
        self.hovered_detail = None;
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('c') => Ok(Command::Cancel),
                KeyCode::Char('t') => self.select_file_open_target(FileOpenTarget::Window),
                KeyCode::Char('v') => self.select_file_open_target(FileOpenTarget::RightPane),
                KeyCode::Char('x') => self.select_file_open_target(FileOpenTarget::BottomPane),
                KeyCode::Char('r') => {
                    if self.window_preview_visible() {
                        self.window_preview_refresh = self.window_preview_refresh.wrapping_add(1);
                    }
                    Ok(self.refresh_command())
                }
                _ => Ok(Command::None),
            };
        }
        if self.input_mode == InputMode::Search {
            return self.handle_search_key(key.code);
        }
        match key.code {
            KeyCode::Esc if self.auxiliary_pane == AuxiliaryPane::Commands => {
                self.toggle_commands();
                Ok(Command::None)
            }
            KeyCode::Esc | KeyCode::Char('q') => Ok(Command::Cancel),
            KeyCode::Left | KeyCode::Char('h') => {
                if self.focus == Focus::Detail
                    && self.right_mode == RightMode::Windows
                    && self.pane_focus
                {
                    self.pane_focus = false;
                } else if self.focus == Focus::Detail && self.right_mode == RightMode::Files {
                    if self.file_focus > 0 {
                        return self.back_file();
                    }
                    if self.cancel_pending_directory_request() {
                        return Ok(Command::None);
                    }
                    self.focus = Focus::Projects;
                } else {
                    self.focus = Focus::Projects;
                }
                Ok(Command::None)
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.focus == Focus::Projects {
                    self.focus = Focus::Detail;
                } else if self.right_mode == RightMode::Files {
                    return self.select_detail();
                } else if self.right_mode == RightMode::Windows
                    && !self.pane_focus
                    && !self.pane_items().is_empty()
                {
                    self.pane_focus = true;
                    self.select_active_pane();
                }
                Ok(Command::None)
            }
            KeyCode::Tab => {
                if self.focus == Focus::Projects {
                    self.focus = Focus::Detail;
                    self.pane_focus = false;
                } else if self.right_mode == RightMode::Files
                    && self.file_focus + 1 < self.file_columns.len()
                {
                    return self.select_detail();
                } else if self.right_mode == RightMode::Windows
                    && !self.pane_focus
                    && !self.pane_items().is_empty()
                {
                    self.pane_focus = true;
                    self.select_active_pane();
                } else {
                    if self.right_mode == RightMode::Files {
                        self.cancel_pending_directory_request();
                    }
                    self.focus = Focus::Projects;
                    self.pane_focus = false;
                }
                Ok(Command::None)
            }
            KeyCode::Enter if self.focus == Focus::Projects => self.drill_project(),
            KeyCode::Enter => self.select_detail(),
            KeyCode::Char('o') => self.select_project(),
            KeyCode::Char('f') => self.show_files(),
            KeyCode::Char('s') => self.show_sessions(),
            KeyCode::Char('w') => {
                self.show_windows();
                Ok(Command::None)
            }
            KeyCode::Char('x') => self.close_selected_project(),
            KeyCode::Char('p') => {
                match self.right_mode {
                    RightMode::Windows => self.toggle_window_preview(),
                    RightMode::Files if self.file_preview_available => {
                        self.file_preview_visible = !self.file_preview_visible;
                    }
                    RightMode::Files | RightMode::Sessions => {
                        self.status = Some("File preview is unavailable".into());
                    }
                }
                Ok(Command::None)
            }
            KeyCode::Char('?') => {
                self.toggle_commands();
                Ok(Command::None)
            }
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Search;
                Ok(Command::None)
            }
            KeyCode::Backspace
                if self.focus == Focus::Detail && self.right_mode == RightMode::Files =>
            {
                self.back_file()
            }
            KeyCode::Backspace
                if self.focus == Focus::Detail
                    && self.right_mode == RightMode::Windows
                    && self.pane_focus =>
            {
                self.pane_focus = false;
                Ok(Command::None)
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_down(),
            _ => Ok(Command::None),
        }
    }

    fn handle_search_key(&mut self, code: KeyCode) -> Result<Command, NavigationError> {
        match code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                Ok(Command::None)
            }
            KeyCode::Backspace => self.edit_query(None),
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Enter if self.focus == Focus::Projects => self.drill_project(),
            KeyCode::Enter => self.select_detail(),
            KeyCode::Char(character) => self.edit_query(Some(character)),
            _ => Ok(Command::None),
        }
    }

    fn edit_query(&mut self, character: Option<char>) -> Result<Command, NavigationError> {
        match self.focus {
            Focus::Projects => {
                let previous = self.selected_target_key();
                if let Some(character) = character {
                    self.project_query.push(character);
                } else {
                    self.project_query.pop();
                }
                self.project_cursor = self
                    .project_cursor
                    .min(self.visible_project_items().len().saturating_sub(1));
                self.project_changed(previous)
            }
            Focus::Detail => {
                if self.right_mode == RightMode::Windows && self.pane_focus {
                    if let Some(character) = character {
                        self.pane_query.push(character);
                    } else {
                        self.pane_query.pop();
                    }
                    self.pane_cursor = self
                        .pane_cursor
                        .min(self.visible_pane_items().len().saturating_sub(1));
                } else if self.right_mode == RightMode::Files {
                    if let Some(column) = self.file_columns.get_mut(self.file_focus) {
                        if let Some(character) = character {
                            column.query.push(character);
                        } else {
                            column.query.pop();
                        }
                    }
                    let item_count = self.visible_detail_items().len();
                    if let Some(column) = self.file_columns.get_mut(self.file_focus) {
                        column.cursor = column.cursor.min(item_count.saturating_sub(1));
                    }
                    return Ok(self.preview_selected_file());
                } else {
                    if let Some(character) = character {
                        self.detail_query.push(character);
                    } else {
                        self.detail_query.pop();
                    }
                    self.detail_cursor = self
                        .detail_cursor
                        .min(self.visible_detail_items().len().saturating_sub(1));
                }
                Ok(Command::None)
            }
        }
    }

    fn move_up(&mut self) -> Result<Command, NavigationError> {
        match self.focus {
            Focus::Projects => {
                let previous = self.selected_target_key();
                self.project_cursor = self.project_cursor.saturating_sub(1);
                self.project_changed(previous)
            }
            Focus::Detail => {
                if self.right_mode == RightMode::Windows && self.pane_focus {
                    self.pane_cursor = self.pane_cursor.saturating_sub(1);
                } else if self.right_mode == RightMode::Files {
                    let previous = self.detail_cursor();
                    if let Some(column) = self.file_columns.get_mut(self.file_focus) {
                        column.cursor = column.cursor.saturating_sub(1);
                    }
                    if self.detail_cursor() != previous {
                        return Ok(self.preview_selected_file());
                    }
                } else {
                    self.detail_cursor = self.detail_cursor.saturating_sub(1);
                    if self.right_mode == RightMode::Windows {
                        self.select_active_pane();
                    }
                }
                Ok(Command::None)
            }
        }
    }

    fn select_project(&self) -> Result<Command, NavigationError> {
        let Some(target) = self.selected_target() else {
            return Ok(Command::None);
        };
        match target {
            ProjectTarget::Project(project) => {
                command_for_outcome(self.navigator.select_project(&project.id, &self.openers)?)
            }
            ProjectTarget::Workspace { name, .. } => {
                Ok(Command::Finish(Selection::Workspace { workspace: name }))
            }
        }
    }

    fn drill_project(&mut self) -> Result<Command, NavigationError> {
        if self.selected_target().is_none() {
            return Ok(Command::None);
        }
        self.focus = Focus::Detail;
        self.pane_focus = false;
        match self.right_mode {
            RightMode::Windows => {
                self.select_active_host_item();
                Ok(Command::None)
            }
            RightMode::Files => {
                if self.current_directory().is_none() {
                    self.show_files()
                } else {
                    Ok(Command::None)
                }
            }
            RightMode::Sessions => Ok(self
                .selected_project_path()
                .map_or(Command::None, Command::LoadSessions)),
        }
    }

    fn select_detail(&mut self) -> Result<Command, NavigationError> {
        let item = if self.right_mode == RightMode::Windows && self.pane_focus {
            self.visible_pane_items().get(self.pane_cursor).cloned()
        } else {
            self.visible_detail_items()
                .get(self.detail_cursor())
                .cloned()
        };
        let Some(item) = item else {
            return Ok(Command::None);
        };
        match item.target {
            DetailTarget::HostWindow(_) => {
                let panes = self.pane_items();
                if panes.len() == 1 && self.single_pane_behavior == SinglePaneBehavior::Activate {
                    if let DetailTarget::HostPane { window_id, pane_id } = &panes[0].target {
                        return self.finish_host_pane(window_id, pane_id);
                    }
                }
                self.pane_focus = true;
                self.select_active_pane();
                Ok(Command::None)
            }
            DetailTarget::HostPane { window_id, pane_id } => {
                self.finish_host_pane(&window_id, &pane_id)
            }
            DetailTarget::Entry(entry) => {
                self.queued_file_preview = None;
                if !entry.kind.is_directory(self.follow_symlinks) {
                    let host_target = self
                        .best_nvim_target(&entry.path)
                        .map(|(host_target, _)| host_target);
                    return command_for_outcome(self.navigator.select_entry(
                        &entry,
                        &self.openers,
                        self.file_open_target,
                        true,
                        host_target,
                    )?);
                }
                let child_depth = self.file_focus.saturating_add(1);
                let child_loaded = self
                    .file_columns
                    .get(child_depth)
                    .is_some_and(|column| column.path == entry.path);
                let outcome = self.navigator.select_entry(
                    &entry,
                    &self.openers,
                    self.file_open_target,
                    true,
                    None,
                )?;
                if child_loaded {
                    self.file_focus = child_depth;
                    Ok(self.preview_selected_file())
                } else {
                    self.pending_file_load = Some((child_depth, entry.path.clone()));
                    self.pending_file_focus = Some(child_depth);
                    command_for_outcome(outcome)
                }
            }
            DetailTarget::OpenCodeSession(id) => {
                if self.session_conflicts.contains(&id) {
                    self.status = Some(format!(
                        "OpenCode session {id} is reported by multiple live servers"
                    ));
                    return Ok(Command::None);
                }
                let Some(project_id) = self.selected_project_id().map(str::to_owned) else {
                    return Ok(Command::None);
                };
                let Some(session) = self
                    .sessions
                    .iter()
                    .find(|session| session.id == id)
                    .cloned()
                else {
                    return Ok(Command::None);
                };
                let Some(command) = self.opencode_command.clone() else {
                    self.status = Some("OpenCode integration is not configured".into());
                    return Ok(Command::None);
                };
                let host_item_id = self
                    .context
                    .session_item(&project_id, &id)
                    .or_else(|| self.session_host_items.get(&id).map(String::as_str))
                    .map(str::to_owned);
                command_for_outcome(self.navigator.select_opencode_session(
                    &project_id,
                    &session,
                    &command,
                    host_item_id.as_deref(),
                )?)
            }
        }
    }

    fn select_file_open_target(
        &mut self,
        open_target: FileOpenTarget,
    ) -> Result<Command, NavigationError> {
        let entry = if self.focus == Focus::Detail && self.right_mode == RightMode::Files {
            self.selected_regular_file().map(|(_, entry)| entry)
        } else {
            None
        };
        let Some(entry) = entry else {
            if self.right_mode == RightMode::Files {
                self.status = Some("Select a file before choosing an open target".into());
            }
            return Ok(Command::None);
        };
        command_for_outcome(self.navigator.select_entry(
            &entry,
            &self.openers,
            open_target,
            false,
            None,
        )?)
    }

    fn finish_host_pane(&self, window_id: &str, pane_id: &str) -> Result<Command, NavigationError> {
        let Some(target) = self.selected_target() else {
            return Ok(Command::None);
        };
        match target {
            ProjectTarget::Project(project) => command_for_outcome(
                self.navigator
                    .select_host_pane(&project.id, window_id, pane_id)?,
            ),
            ProjectTarget::Workspace { name, .. } => {
                Ok(Command::Finish(Selection::WorkspacePane {
                    workspace: name,
                    window_id: window_id.to_string(),
                    pane_id: pane_id.to_string(),
                }))
            }
        }
    }

    fn show_files(&mut self) -> Result<Command, NavigationError> {
        if self.selected_workspace_name().is_some() {
            self.set_workspace_project_status();
            return Ok(Command::None);
        }
        self.focus = Focus::Detail;
        self.right_mode = RightMode::Files;
        self.file_columns.clear();
        self.file_focus = 0;
        self.pending_file_load = None;
        self.pending_file_focus = None;
        self.queued_file_preview = None;
        self.active_directory_request = None;
        self.sessions.clear();
        self.session_host_items.clear();
        self.session_conflicts.clear();
        self.detail_query.clear();
        self.detail_cursor = 0;
        self.pane_query.clear();
        self.pane_cursor = 0;
        self.pane_focus = false;
        let Some(project_id) = self.selected_project_id().map(str::to_owned) else {
            return Ok(Command::None);
        };
        let command = command_for_outcome(self.navigator.browse_project(&project_id)?)?;
        if let Command::LoadDirectory(path) = &command {
            self.pending_file_load = Some((0, path.clone()));
        }
        Ok(command)
    }

    fn show_sessions(&mut self) -> Result<Command, NavigationError> {
        if self.selected_workspace_name().is_some() {
            self.set_workspace_project_status();
            return Ok(Command::None);
        }
        if self.opencode_command.is_none() {
            self.status = Some("OpenCode integration is not configured".into());
            return Ok(Command::None);
        }
        self.focus = Focus::Detail;
        self.right_mode = RightMode::Sessions;
        self.navigator.show_projects();
        self.file_columns.clear();
        self.file_focus = 0;
        self.pending_file_load = None;
        self.pending_file_focus = None;
        self.queued_file_preview = None;
        self.active_directory_request = None;
        self.sessions.clear();
        self.session_host_items.clear();
        self.session_conflicts.clear();
        self.detail_query.clear();
        self.detail_cursor = 0;
        self.pane_query.clear();
        self.pane_cursor = 0;
        self.pane_focus = false;
        self.status = None;
        Ok(self
            .selected_project_path()
            .map_or(Command::None, Command::LoadSessions))
    }

    fn show_windows(&mut self) {
        self.focus = Focus::Detail;
        self.right_mode = RightMode::Windows;
        self.navigator.show_projects();
        self.file_columns.clear();
        self.file_focus = 0;
        self.pending_file_load = None;
        self.pending_file_focus = None;
        self.queued_file_preview = None;
        self.active_directory_request = None;
        self.sessions.clear();
        self.session_host_items.clear();
        self.session_conflicts.clear();
        self.detail_query.clear();
        self.detail_cursor = 0;
        self.pane_query.clear();
        self.pane_cursor = 0;
        self.pane_focus = false;
        self.status = None;
        self.select_active_host_item();
    }

    fn back_file(&mut self) -> Result<Command, NavigationError> {
        if self.cancel_pending_directory_request() {
            return Ok(Command::None);
        }
        if self.file_focus > 0 {
            self.file_focus -= 1;
            let _ = self.navigator.back()?;
            return Ok(Command::None);
        }
        let outcome = self.navigator.back()?;
        if matches!(outcome, NavigationOutcome::Continue) {
            self.focus = Focus::Projects;
        }
        command_for_outcome(outcome)
    }

    fn close_selected_project(&self) -> Result<Command, NavigationError> {
        if self.focus != Focus::Projects {
            return Ok(Command::None);
        }
        let Some(item) = self
            .visible_project_items()
            .get(self.project_cursor)
            .cloned()
        else {
            return Ok(Command::None);
        };
        if !item.host_open {
            return Ok(Command::None);
        }
        match item.target {
            ProjectTarget::Project(project) => {
                command_for_outcome(self.navigator.close_project(&project.id)?)
            }
            ProjectTarget::Workspace { name, .. } => {
                Ok(Command::Finish(Selection::CloseWorkspace {
                    workspace: name,
                }))
            }
        }
    }

    fn move_down(&mut self) -> Result<Command, NavigationError> {
        match self.focus {
            Focus::Projects => {
                let previous = self.selected_target_key();
                self.project_cursor = self
                    .project_cursor
                    .saturating_add(1)
                    .min(self.visible_project_items().len().saturating_sub(1));
                self.project_changed(previous)
            }
            Focus::Detail => {
                if self.right_mode == RightMode::Windows && self.pane_focus {
                    self.pane_cursor = self
                        .pane_cursor
                        .saturating_add(1)
                        .min(self.visible_pane_items().len().saturating_sub(1));
                } else if self.right_mode == RightMode::Files {
                    let item_count = self.visible_detail_items().len();
                    let previous = self.detail_cursor();
                    if let Some(column) = self.file_columns.get_mut(self.file_focus) {
                        column.cursor = column
                            .cursor
                            .saturating_add(1)
                            .min(item_count.saturating_sub(1));
                    }
                    if self.detail_cursor() != previous {
                        return Ok(self.preview_selected_file());
                    }
                } else {
                    self.detail_cursor = self
                        .detail_cursor
                        .saturating_add(1)
                        .min(self.visible_detail_items().len().saturating_sub(1));
                    if self.right_mode == RightMode::Windows {
                        self.select_active_pane();
                    }
                }
                Ok(Command::None)
            }
        }
    }

    fn preview_selected_file(&mut self) -> Command {
        self.cancel_pending_directory_request();
        let depth = self.file_focus;
        self.file_columns.truncate(depth.saturating_add(1));
        let Some(DetailTarget::Entry(entry)) = self
            .visible_detail_items()
            .get(self.detail_cursor())
            .map(|item| item.target.clone())
        else {
            return Command::None;
        };
        if !entry.kind.is_directory(self.follow_symlinks) {
            return Command::None;
        }
        let child_depth = depth.saturating_add(1);
        self.pending_file_load = Some((child_depth, entry.path.clone()));
        Command::LoadDirectory(entry.path)
    }

    fn project_changed(
        &mut self,
        previous: Option<ProjectTargetKey>,
    ) -> Result<Command, NavigationError> {
        let current = self.selected_target_key();
        if current == previous {
            return Ok(Command::None);
        }
        self.file_columns.clear();
        self.file_focus = 0;
        self.pending_file_load = None;
        self.pending_file_focus = None;
        self.queued_file_preview = None;
        self.active_directory_request = None;
        self.sessions.clear();
        self.session_host_items.clear();
        self.session_conflicts.clear();
        self.detail_query.clear();
        self.detail_cursor = 0;
        self.pane_query.clear();
        self.pane_cursor = 0;
        self.pane_focus = false;
        self.status = None;
        if self.selected_workspace_name().is_some() && self.right_mode != RightMode::Windows {
            self.right_mode = RightMode::Windows;
            self.set_workspace_project_status();
            self.select_active_host_item();
            return Ok(Command::None);
        }
        if self.right_mode == RightMode::Files {
            if let Some(project_id) = self.selected_project_id().map(str::to_owned) {
                let command = command_for_outcome(self.navigator.browse_project(&project_id)?)?;
                if let Command::LoadDirectory(path) = &command {
                    self.pending_file_load = Some((0, path.clone()));
                }
                return Ok(command);
            }
        }
        if self.right_mode == RightMode::Sessions {
            return Ok(self
                .selected_project_path()
                .map_or(Command::None, Command::LoadSessions));
        }
        Ok(Command::None)
    }

    fn refresh_command(&self) -> Command {
        if self.focus == Focus::Detail && self.right_mode == RightMode::Files {
            if let Some(path) = self.current_directory() {
                return Command::RefreshDirectory(path.to_path_buf());
            }
        }
        if self.focus == Focus::Detail && self.right_mode == RightMode::Sessions {
            if let Some(path) = self.selected_project_path() {
                return Command::RefreshSessions(path);
            }
        }
        Command::RefreshProjects
    }

    fn reset_queries_and_cursors(&mut self) {
        self.project_query.clear();
        self.detail_query.clear();
        self.project_cursor = 0;
        self.detail_cursor = 0;
        self.pane_query.clear();
        self.pane_cursor = 0;
        self.pane_focus = false;
        self.focus = Focus::Projects;
        self.right_mode = RightMode::Windows;
        self.input_mode = InputMode::Normal;
        self.status = None;
        self.startup_command = None;
    }

    fn select_current_project(&mut self) {
        if let Some(index) = self
            .visible_project_items()
            .iter()
            .position(|item| item.status == ProjectStatus::Current)
        {
            self.project_cursor = index;
        }
    }

    fn selected_project_is_current(&self) -> bool {
        self.visible_project_items()
            .get(self.project_cursor)
            .is_some_and(|item| item.host_current)
    }

    fn selected_project_is_open(&self) -> bool {
        self.visible_project_items()
            .get(self.project_cursor)
            .is_some_and(|item| item.host_open)
    }

    fn select_active_host_item(&mut self) {
        if let Some(index) = self
            .visible_detail_items()
            .iter()
            .position(|item| item.active)
        {
            self.detail_cursor = index;
        }
        self.select_active_pane();
    }

    fn select_active_pane(&mut self) {
        self.pane_query.clear();
        let panes = self.pane_items();
        self.pane_cursor = panes
            .iter()
            .position(|item| item.active)
            .unwrap_or_else(|| self.pane_cursor.min(panes.len().saturating_sub(1)));
    }

    fn visible_project_items(&self) -> Vec<ProjectItem> {
        let mut items = self
            .navigator
            .projects()
            .iter()
            .enumerate()
            .map(|(order, project)| {
                let labels = self.context.labels(&project.id);
                let host_current = labels.iter().any(|label| label == "current");
                ProjectItem {
                    target: ProjectTarget::Project(project.clone()),
                    status: self.project_status(project),
                    host_current,
                    host_open: host_current || labels.iter().any(|label| label == "open"),
                    score: order,
                }
            })
            .collect::<Vec<_>>();
        let project_count = items.len();
        items.extend(self.context.workspaces().iter().enumerate().map(
            |(order, (name, context))| ProjectItem {
                target: ProjectTarget::Workspace {
                    name: name.clone(),
                    context: context.clone(),
                },
                status: if context.current {
                    ProjectStatus::Current
                } else {
                    ProjectStatus::Open
                },
                host_current: context.current,
                host_open: true,
                score: project_count + order,
            },
        ));

        if !self.project_query.is_empty() {
            items.retain_mut(|item| {
                let Some(score) = fuzzy_score(&self.project_query, item.target.display_name())
                else {
                    return false;
                };
                item.score = score;
                true
            });
        }
        items.sort_by_key(|item| (item.status.rank(), item.score));
        items
    }

    fn project_status(&self, project: &Project) -> ProjectStatus {
        let labels = self.context.labels(&project.id);
        let Some(active) = &self.active_project else {
            return ProjectStatus::from_labels(labels);
        };
        if active.project_id == project.id {
            ProjectStatus::Current
        } else {
            ProjectStatus::from_inactive_labels(labels)
        }
    }

    fn visible_detail_items(&self) -> Vec<DetailItem> {
        if self.right_mode == RightMode::Files {
            return self.visible_file_items(self.file_focus);
        }
        let Some(target) = self.selected_target() else {
            return Vec::new();
        };
        let mut items: Vec<DetailItem> = match self.right_mode {
            RightMode::Windows => match &target {
                ProjectTarget::Project(project) => self.context.windows(&project.id),
                ProjectTarget::Workspace { context, .. } => &context.windows,
            }
            .iter()
            .enumerate()
            .map(|(score, item)| DetailItem {
                label: item.label.clone(),
                detail: item.detail.clone(),
                pane_id: item
                    .panes
                    .iter()
                    .find(|pane| pane.active)
                    .or_else(|| item.panes.first())
                    .map(|pane| pane.id.clone()),
                active: item.active,
                score,
                target: DetailTarget::HostWindow(item.id.clone()),
            })
            .collect(),
            RightMode::Files => unreachable!("files return before host target resolution"),
            RightMode::Sessions => {
                let ProjectTarget::Project(project) = &target else {
                    return Vec::new();
                };
                self.ordered_sessions()
                    .into_iter()
                    .enumerate()
                    .map(|(score, (session, depth))| {
                        let conflict = self.session_conflicts.contains(&session.id);
                        let state = if conflict {
                            "conflict: multiple live servers".into()
                        } else {
                            session_state_label(&session)
                        };
                        DetailItem {
                            label: format!("{}{}", "  ".repeat(depth), session.title),
                            detail: Some(format!("{} · {state}", session.type_label())),
                            pane_id: None,
                            active: self
                                .context
                                .session_item(&project.id, &session.id)
                                .is_some()
                                || self.session_host_items.contains_key(&session.id),
                            score,
                            target: DetailTarget::OpenCodeSession(session.id),
                        }
                    })
                    .collect()
            }
        };
        let query = &self.detail_query;
        if !query.is_empty() {
            items.retain_mut(|item| {
                let candidate = item.detail.as_ref().map_or_else(
                    || item.label.clone(),
                    |detail| format!("{} {detail}", item.label),
                );
                let Some(score) = fuzzy_score(query, &candidate) else {
                    return false;
                };
                item.score = score;
                true
            });
            items.sort_by_key(|item| item.score);
        }
        items
    }

    fn visible_file_items(&self, depth: usize) -> Vec<DetailItem> {
        let Some(column) = self.file_columns.get(depth) else {
            return Vec::new();
        };
        let mut items = column
            .entries
            .iter()
            .enumerate()
            .map(|(score, entry)| DetailItem {
                label: format!(
                    "{}{}",
                    entry.name(),
                    if entry.kind.is_directory(self.follow_symlinks) {
                        "/"
                    } else {
                        ""
                    }
                ),
                detail: None,
                pane_id: None,
                active: false,
                score,
                target: DetailTarget::Entry(entry.clone()),
            })
            .collect::<Vec<_>>();
        if !column.query.is_empty() {
            items.retain_mut(|item| {
                let Some(score) = fuzzy_score(&column.query, &item.label) else {
                    return false;
                };
                item.score = score;
                true
            });
            items.sort_by(|left, right| {
                left.score
                    .cmp(&right.score)
                    .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
            });
        }
        items
    }

    fn selected_regular_file(&self) -> Option<(Project, DirectoryEntry)> {
        let ProjectTarget::Project(project) = self.selected_target()? else {
            return None;
        };
        let DetailTarget::Entry(entry) = self
            .visible_file_items(self.file_focus)
            .get(self.detail_cursor())?
            .target
            .clone()
        else {
            return None;
        };
        (entry.kind == wisp_core::model::EntryKind::File).then_some((project, entry))
    }

    fn best_nvim_target(&self, path: &Path) -> Option<(FileHostTarget, NvimView)> {
        let project_id = self.selected_project_id()?;
        let selected_key = comparison_key(&path.to_string_lossy());
        let mut best = None;
        for window in self.context.windows(project_id) {
            for pane in &window.panes {
                for view in &pane.nvim_views {
                    if comparison_key(&view.path.to_string_lossy()) != selected_key {
                        continue;
                    }
                    let rank = (window.active, pane.active, view.active);
                    if best
                        .as_ref()
                        .is_some_and(|(best_rank, _, _)| rank <= *best_rank)
                    {
                        continue;
                    }
                    best = Some((
                        rank,
                        FileHostTarget {
                            window_id: window.id.clone(),
                            pane_id: pane.id.clone(),
                        },
                        view.clone(),
                    ));
                }
            }
        }
        best.map(|(_, host_target, view)| (host_target, view))
    }

    fn visible_pane_items(&self) -> Vec<DetailItem> {
        let mut items = self.pane_items();
        if !self.pane_query.is_empty() {
            items.retain_mut(|item| {
                let candidate = item.detail.as_ref().map_or_else(
                    || item.label.clone(),
                    |detail| format!("{} {detail}", item.label),
                );
                let Some(score) = fuzzy_score(&self.pane_query, &candidate) else {
                    return false;
                };
                item.score = score;
                true
            });
            items.sort_by_key(|item| item.score);
        }
        items
    }

    fn pane_items(&self) -> Vec<DetailItem> {
        if self.right_mode != RightMode::Windows {
            return Vec::new();
        }
        let Some(window_id) = self
            .visible_detail_items()
            .get(self.detail_cursor)
            .and_then(|item| match &item.target {
                DetailTarget::HostWindow(id) => Some(id.clone()),
                DetailTarget::HostPane { .. }
                | DetailTarget::Entry(_)
                | DetailTarget::OpenCodeSession(_) => None,
            })
        else {
            return Vec::new();
        };
        let Some(target) = self.selected_target() else {
            return Vec::new();
        };
        let windows = match &target {
            ProjectTarget::Project(project) => self.context.windows(&project.id),
            ProjectTarget::Workspace { context, .. } => &context.windows,
        };
        let Some(window) = windows.iter().find(|window| window.id == window_id) else {
            return Vec::new();
        };
        window
            .panes
            .iter()
            .enumerate()
            .map(|(score, pane)| DetailItem {
                label: pane.label.clone(),
                detail: pane.detail.clone(),
                pane_id: Some(pane.id.clone()),
                active: pane.active,
                score,
                target: DetailTarget::HostPane {
                    window_id: window.id.clone(),
                    pane_id: pane.id.clone(),
                },
            })
            .collect()
    }

    fn ordered_sessions(&self) -> Vec<(OpenCodeSession, usize)> {
        let sessions = self
            .sessions
            .iter()
            .cloned()
            .map(|session| (session.id.clone(), session))
            .collect::<BTreeMap<_, _>>();
        let mut children = BTreeMap::<Option<String>, Vec<String>>::new();
        for session in sessions.values() {
            let parent = session
                .parent_id
                .as_ref()
                .filter(|parent| sessions.contains_key(*parent) && *parent != &session.id)
                .cloned();
            children.entry(parent).or_default().push(session.id.clone());
        }

        let mut roots = children.remove(&None).unwrap_or_default();
        sort_session_ids(&mut roots, &sessions, &children, true);
        let mut ordered = Vec::new();
        let mut visited = BTreeSet::new();
        for root in roots {
            append_session(&root, 0, &sessions, &children, &mut visited, &mut ordered);
        }
        for id in sessions.keys() {
            if !visited.contains(id) {
                append_session(id, 0, &sessions, &children, &mut visited, &mut ordered);
            }
        }
        ordered
    }
}

fn command_for_outcome(outcome: NavigationOutcome) -> Result<Command, NavigationError> {
    Ok(match outcome {
        NavigationOutcome::Continue => Command::None,
        NavigationOutcome::LoadDirectory { path, .. } => Command::LoadDirectory(path),
        NavigationOutcome::Selected(selection) => Command::Finish(selection),
        NavigationOutcome::Cancelled => Command::Cancel,
    })
}

#[derive(Clone, Debug)]
struct ProjectItem {
    target: ProjectTarget,
    status: ProjectStatus,
    host_current: bool,
    host_open: bool,
    score: usize,
}

#[derive(Clone, Debug)]
enum ProjectTarget {
    Project(Project),
    Workspace {
        name: String,
        context: HostWorkspaceContext,
    },
}

impl ProjectTarget {
    fn display_name(&self) -> &str {
        match self {
            Self::Project(project) => &project.display_name,
            Self::Workspace { name, .. } => name,
        }
    }

    fn key(self) -> ProjectTargetKey {
        match self {
            Self::Project(project) => ProjectTargetKey::Project(project.id),
            Self::Workspace { name, .. } => ProjectTargetKey::Workspace(name),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ProjectTargetKey {
    Project(String),
    Workspace(String),
}

#[derive(Clone, Debug)]
struct DetailItem {
    label: String,
    detail: Option<String>,
    pane_id: Option<String>,
    active: bool,
    score: usize,
    target: DetailTarget,
}

#[derive(Clone, Debug)]
struct FileColumn {
    path: PathBuf,
    entries: Vec<DirectoryEntry>,
    cursor: usize,
    query: String,
}

#[derive(Clone, Debug)]
enum DetailTarget {
    HostWindow(String),
    HostPane { window_id: String, pane_id: String },
    Entry(DirectoryEntry),
    OpenCodeSession(String),
}

fn append_session(
    id: &str,
    depth: usize,
    sessions: &BTreeMap<String, OpenCodeSession>,
    children: &BTreeMap<Option<String>, Vec<String>>,
    visited: &mut BTreeSet<String>,
    ordered: &mut Vec<(OpenCodeSession, usize)>,
) {
    if !visited.insert(id.to_string()) {
        return;
    }
    let Some(session) = sessions.get(id) else {
        return;
    };
    ordered.push((session.clone(), depth));
    let mut child_ids = children
        .get(&Some(id.to_string()))
        .cloned()
        .unwrap_or_default();
    sort_session_ids(&mut child_ids, sessions, children, false);
    for child in child_ids {
        append_session(
            &child,
            depth.saturating_add(1),
            sessions,
            children,
            visited,
            ordered,
        );
    }
}

fn sort_session_ids(
    ids: &mut [String],
    sessions: &BTreeMap<String, OpenCodeSession>,
    children: &BTreeMap<Option<String>, Vec<String>>,
    roots: bool,
) {
    ids.sort_by(|left, right| {
        let left_session = &sessions[left];
        let right_session = &sessions[right];
        let left_summary = if roots {
            subtree_summary(left, sessions, children, &mut BTreeSet::new())
        } else {
            (session_priority(left_session), left_session.updated_at)
        };
        let right_summary = if roots {
            subtree_summary(right, sessions, children, &mut BTreeSet::new())
        } else {
            (session_priority(right_session), right_session.updated_at)
        };
        left_summary
            .0
            .cmp(&right_summary.0)
            .then_with(|| right_summary.1.cmp(&left_summary.1))
            .then_with(|| left_session.title.cmp(&right_session.title))
            .then_with(|| left.cmp(right))
    });
}

fn subtree_summary(
    id: &str,
    sessions: &BTreeMap<String, OpenCodeSession>,
    children: &BTreeMap<Option<String>, Vec<String>>,
    visited: &mut BTreeSet<String>,
) -> (usize, u64) {
    if !visited.insert(id.to_string()) {
        return (usize::MAX, 0);
    }
    let Some(session) = sessions.get(id) else {
        return (usize::MAX, 0);
    };
    let mut result = (session_priority(session), session.updated_at);
    if let Some(child_ids) = children.get(&Some(id.to_string())) {
        for child in child_ids {
            let summary = subtree_summary(child, sessions, children, visited);
            result.0 = result.0.min(summary.0);
            result.1 = result.1.max(summary.1);
        }
    }
    result
}

fn session_priority(session: &OpenCodeSession) -> usize {
    match session.display_state() {
        SessionDisplayState::Waiting { questions, .. } if questions > 0 => 0,
        SessionDisplayState::Waiting { .. } => 1,
        SessionDisplayState::Retrying { .. } => 2,
        SessionDisplayState::Running => 3,
        SessionDisplayState::Idle => 4,
        SessionDisplayState::Error { .. } => 5,
    }
}

fn session_state_label(session: &OpenCodeSession) -> String {
    match session.display_state() {
        SessionDisplayState::Waiting {
            permissions,
            questions,
        } => match (questions, permissions) {
            (questions, permissions) if questions > 0 && permissions > 0 => {
                format!("waiting: {questions} question(s), {permissions} permission(s)")
            }
            (questions, _) if questions > 0 => format!("waiting: {questions} question(s)"),
            (_, permissions) => format!("waiting: {permissions} permission(s)"),
        },
        SessionDisplayState::Retrying {
            attempt, message, ..
        } => format!("retrying #{attempt}: {message}"),
        SessionDisplayState::Running => "running".into(),
        SessionDisplayState::Idle => "idle".into(),
        SessionDisplayState::Error { message } => format!("error: {message}"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectStatus {
    Current,
    Open,
    New,
    Unknown,
}

impl ProjectStatus {
    fn from_labels(labels: &[String]) -> Self {
        if labels.iter().any(|label| label == "current") {
            Self::Current
        } else if labels.iter().any(|label| label == "open") {
            Self::Open
        } else if labels.iter().any(|label| label == "new") {
            Self::New
        } else {
            Self::Unknown
        }
    }

    fn from_inactive_labels(labels: &[String]) -> Self {
        if labels.iter().any(|label| label == "open") {
            Self::Open
        } else if labels.iter().any(|label| label == "new") {
            Self::New
        } else {
            Self::Unknown
        }
    }

    const fn rank(self) -> usize {
        match self {
            Self::Current => 0,
            Self::Open => 1,
            Self::New => 2,
            Self::Unknown => 3,
        }
    }

    const fn icon(self) -> &'static str {
        match self {
            Self::Current => "◆",
            Self::Open => "●",
            Self::New => "○",
            Self::Unknown => "·",
        }
    }

    const fn color(self) -> Color {
        match self {
            Self::Current => THEME.active,
            Self::Open => THEME.accent,
            Self::New | Self::Unknown => THEME.muted,
        }
    }
}

fn fuzzy_score(query: &str, candidate: &str) -> Option<usize> {
    let query = query.to_lowercase();
    let candidate = candidate.to_lowercase();
    let mut candidate_chars = candidate.char_indices();
    let mut previous = None;
    let mut score = 0;

    for query_char in query.chars() {
        let (index, _) =
            candidate_chars.find(|(_, candidate_char)| *candidate_char == query_char)?;
        score += match previous {
            Some(previous) => index.saturating_sub(previous + 1),
            None => index,
        };
        previous = Some(index);
    }
    Some(score)
}

fn truncate_left(value: &str, width: usize) -> Option<String> {
    if width == 0 {
        return None;
    }
    if Span::raw(value).width() <= width {
        return Some(value.to_string());
    }
    if width == 1 {
        return Some("…".into());
    }
    value
        .char_indices()
        .map(|(index, _)| &value[index..])
        .find(|suffix| Span::raw(*suffix).width().saturating_add(1) <= width)
        .map(|suffix| format!("…{suffix}"))
        .or_else(|| Some("…".into()))
}

fn truncate_right(value: &str, width: usize) -> Option<String> {
    if width == 0 {
        return None;
    }
    if Span::raw(value).width() <= width {
        return Some(value.to_string());
    }
    if width == 1 {
        return Some("…".into());
    }
    let mut end = value.len();
    while end > 0 {
        let candidate = &value[..end];
        if Span::raw(candidate).width().saturating_add(1) <= width {
            return Some(format!("{candidate}…"));
        }
        end = value[..end]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index);
    }
    Some("…".into())
}

#[derive(Clone)]
struct VcsToken {
    text: String,
    color: Color,
}

fn push_counted_vcs_token(
    tokens: &mut Vec<VcsToken>,
    icon: &Option<String>,
    count: usize,
    color: Color,
) {
    if count == 0 {
        return;
    }
    if let Some(icon) = icon {
        tokens.push(VcsToken {
            text: format!("{icon}{count}"),
            color,
        });
    }
}

fn vcs_tokens(summary: &GitSummary, icons: &VcsIcons) -> Vec<VcsToken> {
    let mut tokens = Vec::new();
    let state = if summary.dirty {
        icons.dirty.as_ref().map(|icon| (icon, THEME.input))
    } else {
        icons.clean.as_ref().map(|icon| (icon, THEME.active))
    };
    if let Some((icon, color)) = state {
        tokens.push(VcsToken {
            text: icon.clone(),
            color,
        });
    }

    push_counted_vcs_token(
        &mut tokens,
        &icons.untracked,
        summary.untracked,
        THEME.input,
    );
    push_counted_vcs_token(&mut tokens, &icons.modified, summary.modified, THEME.input);
    push_counted_vcs_token(&mut tokens, &icons.staged, summary.staged, THEME.input);
    push_counted_vcs_token(
        &mut tokens,
        &icons.conflicted,
        summary.conflicted,
        THEME.error,
    );
    if summary.ahead > 0 && summary.behind > 0 {
        if let Some(icon) = &icons.diverged {
            tokens.push(VcsToken {
                text: format!("{icon}{}/{}", summary.ahead, summary.behind),
                color: THEME.accent,
            });
        } else {
            push_counted_vcs_token(&mut tokens, &icons.ahead, summary.ahead, THEME.accent);
            push_counted_vcs_token(&mut tokens, &icons.behind, summary.behind, THEME.accent);
        }
    } else {
        push_counted_vcs_token(&mut tokens, &icons.ahead, summary.ahead, THEME.accent);
        push_counted_vcs_token(&mut tokens, &icons.behind, summary.behind, THEME.accent);
    }
    push_counted_vcs_token(&mut tokens, &icons.stashed, summary.stashed, THEME.muted);
    tokens
}

fn vcs_tokens_width(tokens: &[VcsToken]) -> usize {
    tokens
        .iter()
        .map(|token| Span::raw(&token.text).width())
        .sum::<usize>()
        .saturating_add(tokens.len().saturating_sub(1))
}

fn full_vcs_width(summary: &GitSummary, icons: &VcsIcons) -> usize {
    let tokens = vcs_tokens(summary, icons);
    Span::raw(&summary.branch)
        .width()
        .saturating_add((!tokens.is_empty()) as usize)
        .saturating_add(vcs_tokens_width(&tokens))
}

fn vcs_spans(summary: &GitSummary, icons: &VcsIcons, width: usize) -> (Vec<Span<'static>>, usize) {
    if width == 0 {
        return (Vec::new(), 0);
    }

    let tokens = vcs_tokens(summary, icons);
    let primary_token_reservation = tokens
        .first()
        .map_or(0, |token| Span::raw(&token.text).width().saturating_add(1));
    let branch_preference = Span::raw(&summary.branch)
        .width()
        .min(8)
        .min(width.saturating_sub(primary_token_reservation));
    let token_budget = width.saturating_sub(branch_preference.saturating_add(1));
    let mut selected = Vec::new();
    let mut selected_width: usize = 0;
    let mut omitted = false;
    for token in tokens {
        let token_width = Span::raw(&token.text).width();
        let additional = token_width.saturating_add((!selected.is_empty()) as usize);
        if selected_width.saturating_add(additional) <= token_budget {
            selected_width = selected_width.saturating_add(additional);
            selected.push(token);
        } else {
            omitted = true;
        }
    }
    if omitted {
        let additional = 1_usize.saturating_add((!selected.is_empty()) as usize);
        while selected_width.saturating_add(additional) > token_budget && selected.len() > 1 {
            selected.pop();
            selected_width = vcs_tokens_width(&selected);
        }
        if selected_width.saturating_add(additional) <= token_budget {
            selected.push(VcsToken {
                text: "…".into(),
                color: THEME.muted,
            });
            selected_width = vcs_tokens_width(&selected);
        }
    }

    let separator_width = (!selected.is_empty()) as usize;
    let branch_width = width
        .saturating_sub(selected_width)
        .saturating_sub(separator_width);
    let branch = truncate_right(&summary.branch, branch_width);
    let mut spans = Vec::new();
    if let Some(branch) = branch {
        spans.push(Span::styled(branch, Style::default().fg(THEME.accent)));
    }
    if !spans.is_empty() && !selected.is_empty() {
        spans.push(Span::raw(" "));
    }
    for (index, token) in selected.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(token.text, Style::default().fg(token.color)));
    }
    let rendered_width = Line::from(spans.clone()).width();
    (spans, rendered_width)
}

fn project_row(
    item: ProjectItem,
    active: Option<&ActiveProjectContext>,
    git: Option<&GitSummary>,
    vcs_icons: &VcsIcons,
    width: usize,
) -> ListItem<'static> {
    const GIT_GAP: usize = 2;
    const COMPACT_PROJECT_NAME_WIDTH: usize = 15;

    let display_name = item.target.display_name();
    let status_width = Span::raw(item.status.icon()).width().saturating_add(1);
    let full_name_width = Span::raw(display_name).width();
    let name_width = git.map_or_else(
        || width.saturating_sub(status_width),
        |git| {
            let full_git_width = full_vcs_width(git, vcs_icons);
            let preferred_name_width = full_name_width.min(COMPACT_PROJECT_NAME_WIDTH).min(
                width
                    .saturating_sub(status_width)
                    .saturating_sub(GIT_GAP)
                    .saturating_sub(1),
            );
            let available_git_width = width
                .saturating_sub(status_width)
                .saturating_sub(GIT_GAP)
                .saturating_sub(preferred_name_width);
            if full_git_width <= available_git_width {
                full_name_width.min(
                    width
                        .saturating_sub(status_width)
                        .saturating_sub(GIT_GAP)
                        .saturating_sub(full_git_width),
                )
            } else {
                preferred_name_width
            }
        },
    );
    let display_name = truncate_right(display_name, name_width).unwrap_or_default();
    let mut spans = vec![
        Span::styled(item.status.icon(), Style::default().fg(item.status.color())),
        Span::raw(" "),
        Span::raw(display_name),
    ];
    let base_width = Line::from(spans.clone()).width();

    let git = git.map(|git| {
        let available = width.saturating_sub(base_width).saturating_sub(GIT_GAP);
        vcs_spans(git, vcs_icons, available)
    });
    let git_width = git.as_ref().map_or(0, |(_, width)| *width);
    let file_width = width
        .saturating_sub(base_width)
        .saturating_sub(git_width)
        .saturating_sub(if git.is_some() { GIT_GAP } else { 0 })
        .saturating_sub(2);
    if let Some(file) = active
        .and_then(|active| active.file.as_deref())
        .and_then(|file| truncate_left(file, file_width))
    {
        spans.push(Span::styled(
            format!("  {file}"),
            Style::default().fg(THEME.muted),
        ));
    }
    if let Some((git_spans, _)) = git {
        let padding = width
            .saturating_sub(Line::from(spans.clone()).width())
            .saturating_sub(git_width);
        spans.push(Span::raw(" ".repeat(padding)));
        spans.extend(git_spans);
    }
    ListItem::new(Line::from(spans))
}

#[derive(Clone, Copy)]
struct UiAreas {
    projects: Rect,
    detail: Option<Rect>,
    auxiliary: Option<Rect>,
    utility: Rect,
}

fn inset(area: Rect, amount: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(amount),
        area.y.saturating_add(amount),
        area.width.saturating_sub(amount.saturating_mul(2)),
        area.height.saturating_sub(amount.saturating_mul(2)),
    )
}

fn list_offset(selected: usize, item_count: usize, height: usize) -> usize {
    if height == 0 || item_count <= height {
        0
    } else {
        selected
            .saturating_sub(height.saturating_sub(1))
            .min(item_count.saturating_sub(height))
    }
}

fn ui_areas(area: Rect, auxiliary_visible: bool) -> UiAreas {
    let [content, utility] =
        Layout::vertical([Constraint::Min(4), Constraint::Length(3)]).areas(area);
    let projects_width = (content.width / 3).clamp(32, 56);
    let stack_content = content.width < 72
        || (auxiliary_visible && content.width.saturating_sub(projects_width) < 48);
    let (projects, detail, auxiliary) = if stack_content {
        let [projects, remaining] =
            Layout::vertical([Constraint::Percentage(40), Constraint::Percentage(60)])
                .areas(content);
        if auxiliary_visible && remaining.height >= 8 {
            let [detail, auxiliary] =
                Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .areas(remaining);
            (projects, Some(detail), Some(auxiliary))
        } else if auxiliary_visible {
            (projects, None, Some(remaining))
        } else {
            (projects, Some(remaining), None)
        }
    } else {
        let [projects, remaining] =
            Layout::horizontal([Constraint::Length(projects_width), Constraint::Min(0)])
                .areas(content);
        if auxiliary_visible && remaining.width >= 48 {
            let [detail, auxiliary] =
                Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                    .areas(remaining);
            (projects, Some(detail), Some(auxiliary))
        } else if auxiliary_visible {
            (projects, None, Some(remaining))
        } else {
            (projects, Some(remaining), None)
        }
    };
    UiAreas {
        projects,
        detail,
        auxiliary,
        utility,
    }
}

fn window_pane_areas(area: Rect, pane_focus: bool) -> (Option<Rect>, Option<Rect>) {
    if area.width >= 40 {
        let [windows, panes] =
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                .areas(area);
        (Some(windows), Some(panes))
    } else if pane_focus {
        (None, Some(area))
    } else {
        (Some(area), None)
    }
}

fn visible_file_column_depths(app: &App, max_columns: usize) -> Vec<usize> {
    let count = app.file_columns.len().min(max_columns.max(1));
    if count == 0 {
        return Vec::new();
    }
    let end = app
        .file_focus
        .saturating_add(2)
        .max(count)
        .min(app.file_columns.len());
    (end.saturating_sub(count)..end).collect()
}

fn render_file_column(frame: &mut Frame, app: &App, depth: usize, area: Rect) {
    let Some(column) = app.file_columns.get(depth) else {
        frame.render_widget(
            Paragraph::new("No files")
                .style(Style::default().fg(THEME.muted))
                .block(Block::default().borders(Borders::ALL).title(" Files ")),
            area,
        );
        return;
    };
    let title = column
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| column.path.display().to_string());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(
            if app.focus == Focus::Detail && app.file_focus == depth {
                THEME.accent
            } else {
                THEME.muted
            },
        ))
        .title(format!(" {title} "));
    let rows = app
        .visible_file_items(depth)
        .into_iter()
        .map(|item| {
            let live = match &item.target {
                DetailTarget::Entry(entry) if entry.kind == wisp_core::model::EntryKind::File => {
                    app.best_nvim_target(&entry.path).is_some()
                }
                _ => false,
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    if live { "◆" } else { " " },
                    Style::default().fg(if live { THEME.active } else { THEME.muted }),
                ),
                Span::raw(" "),
                Span::raw(item.label),
            ]))
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(if column.query.is_empty() {
                "No files"
            } else {
                "No matching files"
            })
            .style(Style::default().fg(THEME.muted))
            .block(block),
            area,
        );
        return;
    }
    let cursor = column.cursor.min(rows.len().saturating_sub(1));
    let offset = list_offset(cursor, rows.len(), inset(area, 1).height as usize);
    let mut state = ListState::default()
        .with_selected(Some(cursor))
        .with_offset(offset);
    frame.render_stateful_widget(
        List::new(rows)
            .block(block)
            .highlight_symbol("> ")
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)),
        area,
        &mut state,
    );
}

pub fn render(frame: &mut Frame, app: &App) {
    let auxiliary_visible =
        app.auxiliary_pane == AuxiliaryPane::Commands || app.window_preview_visible();
    let layout = ui_areas(frame.area(), auxiliary_visible);
    let projects_area = layout.projects;
    let (detail_area, pane_area) = match (app.right_mode, layout.detail) {
        (RightMode::Files, _) => (None, None),
        (RightMode::Windows, Some(area)) => window_pane_areas(area, app.pane_focus),
        (_, area) => (area, None),
    };
    let auxiliary_area = layout.auxiliary;
    let utility_area = layout.utility;

    let project_rows = app
        .visible_project_items()
        .into_iter()
        .map(|item| {
            let (active, git) = match &item.target {
                ProjectTarget::Project(project) => (
                    app.active_project
                        .as_ref()
                        .filter(|active| active.project_id == project.id),
                    app.project_git.get(&project.id),
                ),
                ProjectTarget::Workspace { .. } => (None, None),
            };
            project_row(
                item,
                active,
                git,
                &app.vcs_icons,
                projects_area.width.saturating_sub(4).into(),
            )
        })
        .collect::<Vec<_>>();
    let mut project_state = ListState::default()
        .with_selected((!project_rows.is_empty()).then_some(app.project_cursor));
    frame.render_stateful_widget(
        List::new(project_rows)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(if app.focus == Focus::Projects {
                        THEME.accent
                    } else {
                        THEME.muted
                    }))
                    .title(" Projects "),
            )
            .highlight_symbol("> ")
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)),
        projects_area,
        &mut project_state,
    );

    if app.right_mode == RightMode::Files {
        if let Some(area) = layout.detail {
            let target_columns = if frame.area().width >= 160 {
                3
            } else if frame.area().width >= 96 {
                2
            } else {
                1
            };
            let max_columns = target_columns.min((area.width / 24).max(1) as usize);
            let depths = visible_file_column_depths(app, max_columns);
            if depths.is_empty() {
                render_file_column(frame, app, 0, area);
            } else {
                let constraints = vec![Constraint::Fill(1); depths.len()];
                let areas = Layout::horizontal(constraints).split(area);
                for (depth, area) in depths.into_iter().zip(areas.iter().copied()) {
                    render_file_column(frame, app, depth, area);
                }
            }
        }
    }

    let detail_rows = app
        .visible_detail_items()
        .into_iter()
        .map(|item| {
            let mut spans = vec![
                Span::styled(
                    if item.active { "◆" } else { " " },
                    Style::default().fg(if item.active {
                        THEME.active
                    } else {
                        THEME.muted
                    }),
                ),
                Span::raw(" "),
                Span::raw(item.label),
            ];
            if let Some(detail) = item.detail {
                spans.push(Span::styled(
                    format!("  {detail}"),
                    Style::default().fg(THEME.muted),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect::<Vec<_>>();
    if let Some(detail_area) = detail_area {
        let detail_block = Block::default()
            .borders(Borders::ALL)
            .border_style(
                Style::default().fg(if app.focus == Focus::Detail && !app.pane_focus {
                    THEME.accent
                } else {
                    THEME.muted
                }),
            )
            .title(match app.right_mode {
                RightMode::Windows => " Windows ",
                RightMode::Files => " Files ",
                RightMode::Sessions => " Sessions ",
            });
        if detail_rows.is_empty() {
            frame.render_widget(
                Paragraph::new(match app.right_mode {
                    RightMode::Windows if !app.selected_project_is_open() => "Project is not open",
                    RightMode::Windows if !app.detail_query.is_empty() => "No matching windows",
                    RightMode::Windows => "No windows",
                    RightMode::Files if !app.detail_query().is_empty() => "No matching files",
                    RightMode::Files => "No files",
                    RightMode::Sessions if !app.detail_query.is_empty() => "No matching sessions",
                    RightMode::Sessions => "No OpenCode sessions",
                })
                .style(Style::default().fg(THEME.muted))
                .block(detail_block),
                detail_area,
            );
        } else {
            let detail_offset = list_offset(
                app.detail_cursor,
                detail_rows.len(),
                inset(detail_area, 1).height as usize,
            );
            let mut detail_state = ListState::default()
                .with_selected(Some(app.detail_cursor))
                .with_offset(detail_offset);
            frame.render_stateful_widget(
                List::new(detail_rows)
                    .block(detail_block)
                    .highlight_symbol("> ")
                    .highlight_style(
                        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
                    ),
                detail_area,
                &mut detail_state,
            );
        }
    }

    if let Some(pane_area) = pane_area {
        let pane_rows = app
            .visible_pane_items()
            .into_iter()
            .map(|item| {
                let mut spans = vec![
                    Span::styled(
                        if item.active { "◆" } else { " " },
                        Style::default().fg(if item.active {
                            THEME.active
                        } else {
                            THEME.muted
                        }),
                    ),
                    Span::raw(" "),
                    Span::raw(item.label),
                ];
                if let Some(detail) = item.detail {
                    spans.push(Span::styled(
                        format!("  {detail}"),
                        Style::default().fg(THEME.muted),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect::<Vec<_>>();
        let pane_block = Block::default()
            .borders(Borders::ALL)
            .border_style(
                Style::default().fg(if app.focus == Focus::Detail && app.pane_focus {
                    THEME.accent
                } else {
                    THEME.muted
                }),
            )
            .title(" Panes ");
        if pane_rows.is_empty() {
            frame.render_widget(
                Paragraph::new(if app.pane_query.is_empty() {
                    "No panes"
                } else {
                    "No matching panes"
                })
                .style(Style::default().fg(THEME.muted))
                .block(pane_block),
                pane_area,
            );
        } else {
            let pane_offset = list_offset(
                app.pane_cursor,
                pane_rows.len(),
                inset(pane_area, 1).height as usize,
            );
            let mut pane_state = ListState::default()
                .with_selected(Some(app.pane_cursor))
                .with_offset(pane_offset);
            frame.render_stateful_widget(
                List::new(pane_rows)
                    .block(pane_block)
                    .highlight_symbol("> ")
                    .highlight_style(
                        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
                    ),
                pane_area,
                &mut pane_state,
            );
        }
    }

    if let Some(auxiliary_area) = auxiliary_area {
        if app.auxiliary_pane == AuxiliaryPane::Commands {
            frame.render_widget(
                Paragraph::new(
                    "↑/↓ j/k Move   h/l Tab Focus\n\
                     Enter Default/Select   Ctrl-T Window\n\
                     Ctrl-V Right   Ctrl-X Bottom\n\
                     w/f/s View   / Search\n\
                     Backspace Parent   p File/Window Preview\n\
                     Ctrl-R Refresh   x Close\n\
                     q/Ctrl-C Cancel   ?/Esc Close Help",
                )
                .style(Style::default().fg(THEME.muted))
                .block(Block::default().borders(Borders::ALL).title(" Commands ")),
                auxiliary_area,
            );
        } else {
            let preview = match &app.window_preview_state {
                WindowPreviewState::Disabled => "Preview disabled".to_string(),
                WindowPreviewState::Loading => "Loading preview".to_string(),
                WindowPreviewState::Ready(text) => text.clone(),
                WindowPreviewState::NoOutput => "No output".to_string(),
                WindowPreviewState::Unavailable => "Preview unavailable".to_string(),
            };
            frame.render_widget(
                Paragraph::new(preview)
                    .style(Style::default().fg(THEME.muted))
                    .block(Block::default().borders(Borders::ALL).title(" Preview ")),
                auxiliary_area,
            );
        }
    }

    let view = match app.right_mode {
        RightMode::Windows => "Windows",
        RightMode::Files => "Files",
        RightMode::Sessions => "Sessions",
    };
    let focused = match app.focus {
        Focus::Projects => "Projects".to_string(),
        Focus::Detail if app.right_mode == RightMode::Windows && app.pane_focus => {
            "Panes".to_string()
        }
        Focus::Detail if app.right_mode == RightMode::Files => app
            .file_columns
            .get(app.file_focus)
            .and_then(|column| column.path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Files".into()),
        Focus::Detail => view.to_string(),
    };
    let query = match app.focus {
        Focus::Projects => &app.project_query,
        Focus::Detail if app.right_mode == RightMode::Windows && app.pane_focus => &app.pane_query,
        Focus::Detail => app.detail_query(),
    };
    let (utility_text, utility_style) = if app.input_mode == InputMode::Search {
        (
            format!("SEARCH  {focused}  / {query}"),
            Style::default().fg(THEME.input),
        )
    } else if let Some(status) = &app.status {
        (format!("ERROR  {status}"), Style::default().fg(THEME.error))
    } else {
        (
            if app.right_mode == RightMode::Windows && app.pane_focus {
                "NORMAL  Projects > Windows > Panes".to_string()
            } else if app.right_mode == RightMode::Files {
                format!("NORMAL  Projects > {}", app.file_breadcrumb())
            } else {
                format!("NORMAL  Projects > {view}")
            },
            Style::default().fg(THEME.muted),
        )
    };
    frame.render_widget(
        Paragraph::new(utility_text).style(utility_style).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::White)),
        ),
        utility_area,
    );
}

pub fn run<D: DataSource>(mut app: App, data: &mut D) -> Result<Option<Selection>, TuiError> {
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    enable_raw_mode()?;
    if let Err(error) = execute!(terminal.backend_mut(), EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(error.into());
    }
    let mut input = CrosstermInput::default();
    let result = run_with_terminal(&mut terminal, &mut app, data, &mut input);
    let leave_result = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let raw_result = disable_raw_mode();
    let cursor_result = terminal.show_cursor();

    match result {
        Err(error) => Err(error),
        Ok(selection) => {
            leave_result?;
            raw_result?;
            cursor_result?;
            Ok(selection)
        }
    }
}

fn issue_directory_request<D: DataSource>(
    app: &mut App,
    data: &mut D,
    request_id: &mut u64,
    path: PathBuf,
    refresh: bool,
) {
    *request_id = request_id.wrapping_add(1);
    let request = DirectoryRequest {
        request_id: *request_id,
        path,
        refresh,
    };
    app.begin_directory_request(&request);
    if let Some(update) = data.request_directory(request) {
        app.apply_directory_update(update);
    }
}

const FILE_PREVIEW_DEBOUNCE: Duration = Duration::from_millis(40);
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Default)]
struct FilePreviewPublication {
    published: Option<FilePreviewState>,
    pending: Option<(FilePreviewState, Instant)>,
}

impl FilePreviewPublication {
    fn observe(&mut self, state: FilePreviewState, now: Instant) -> Option<FilePreviewState> {
        if self.published.as_ref() == Some(&state) {
            self.pending = None;
            return None;
        }
        if matches!(state, FilePreviewState::File { .. }) {
            if self
                .pending
                .as_ref()
                .is_none_or(|(pending, _)| pending != &state)
            {
                self.pending = Some((state, now + FILE_PREVIEW_DEBOUNCE));
            }
            return None;
        }
        self.pending = None;
        self.published = Some(state.clone());
        Some(state)
    }

    fn take_due(&mut self, now: Instant) -> Option<FilePreviewState> {
        if self
            .pending
            .as_ref()
            .is_none_or(|(_, deadline)| *deadline > now)
        {
            return None;
        }
        let (state, _) = self.pending.take()?;
        self.published = Some(state.clone());
        Some(state)
    }

    fn input_timeout(&self, now: Instant) -> Duration {
        self.pending
            .as_ref()
            .map_or(INPUT_POLL_INTERVAL, |(_, deadline)| {
                deadline
                    .saturating_duration_since(now)
                    .min(INPUT_POLL_INTERVAL)
            })
    }
}

pub fn run_with_terminal<B, D, I>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    data: &mut D,
    input: &mut I,
) -> Result<Option<Selection>, TuiError>
where
    B: Backend,
    D: DataSource,
    I: Input,
{
    let mut mouse_capture_enabled = false;
    let mut file_preview_publication = FilePreviewPublication::default();
    let result = (|| -> Result<Option<Selection>, TuiError> {
        let mut requested_preview_target = None;
        let mut preview_request_id = 0_u64;
        let mut directory_request_id = 0_u64;
        if let Some(Command::LoadSessions(path)) = app.take_startup_command() {
            match data.sessions(&path) {
                Ok(snapshot) => app.load_sessions(snapshot),
                Err(error) => app.set_status(error),
            }
        }
        loop {
            let file_preview = app.file_preview_state();
            let now = Instant::now();
            let publication = file_preview_publication
                .observe(file_preview, now)
                .or_else(|| file_preview_publication.take_due(now));
            if let Some(publication) = publication {
                if let Err(error) = data.publish_file_preview(publication) {
                    app.set_status(error);
                }
            }
            while let Some(update) = data.directory_update() {
                app.apply_directory_update(update);
            }
            if let Some(path) = app.take_file_preview_request() {
                issue_directory_request(app, data, &mut directory_request_id, path, false);
            }
            let desired_mouse_capture = app.window_preview_visible();
            if desired_mouse_capture != mouse_capture_enabled {
                input.set_mouse_capture(desired_mouse_capture)?;
                mouse_capture_enabled = desired_mouse_capture;
            }
            let preview_target = app
                .window_preview_target()
                .map(|pane_id| (pane_id, app.window_preview_refresh));
            if preview_target != requested_preview_target {
                requested_preview_target = preview_target.clone();
                if let Some((pane_id, _)) = preview_target {
                    preview_request_id = preview_request_id.wrapping_add(1);
                    let request = WindowPreviewRequest {
                        request_id: preview_request_id,
                        pane_id,
                    };
                    app.begin_window_preview(&request);
                    data.request_window_preview(request);
                } else {
                    data.cancel_window_preview();
                    app.clear_window_preview();
                }
            }
            if let Some(update) = data.window_preview_update() {
                app.apply_window_preview(update);
            }
            for (project_id, git) in data.project_git_updates() {
                app.set_active_project_git(&project_id, git);
            }
            if app.right_mode == RightMode::Sessions && data.session_updates_pending() {
                if let Some(path) = app.selected_project_path() {
                    match data.sessions(&path) {
                        Ok(snapshot) => app.load_sessions(snapshot),
                        Err(error) => app.set_status(error),
                    }
                }
            }
            terminal.draw(|frame| render(frame, app))?;
            let Some(event) =
                input.read_event_timeout(file_preview_publication.input_timeout(Instant::now()))?
            else {
                continue;
            };
            let command = match event {
                InputEvent::Key(key) => app.handle_key(key)?,
                InputEvent::Mouse(mouse) => {
                    let size = terminal.size()?;
                    app.handle_mouse(mouse, Rect::new(0, 0, size.width, size.height));
                    Command::None
                }
                InputEvent::Resize => Command::None,
            };
            match command {
                Command::None => {}
                Command::LoadDirectory(path) => {
                    issue_directory_request(app, data, &mut directory_request_id, path, false)
                }
                Command::LoadSessions(path) => match data.sessions(&path) {
                    Ok(snapshot) => app.load_sessions(snapshot),
                    Err(error) => app.set_status(error),
                },
                Command::RefreshProjects => {
                    let preview_target = app.window_preview_target();
                    let preview_focus = app.focus;
                    let preview_pane_focus = app.pane_focus;
                    match data.refresh_projects() {
                        Ok(projects) => {
                            app.replace_projects(projects);
                            if preview_target.as_deref().is_some_and(|pane_id| {
                                app.restore_window_preview_target(pane_id, preview_pane_focus)
                            }) {
                                app.focus = preview_focus;
                            }
                        }
                        Err(error) => app.set_status(error),
                    }
                }
                Command::RefreshDirectory(path) => {
                    issue_directory_request(app, data, &mut directory_request_id, path, true)
                }
                Command::RefreshSessions(path) => match data.refresh_sessions(&path) {
                    Ok(snapshot) => app.load_sessions(snapshot),
                    Err(error) => app.set_status(error),
                },
                Command::Finish(selection) => return Ok(Some(selection)),
                Command::Cancel => return Ok(None),
            }
        }
    })();
    let mouse_result = if mouse_capture_enabled {
        input.set_mouse_capture(false)
    } else {
        Ok(())
    };
    if file_preview_publication.published.as_ref() != Some(&FilePreviewState::Hidden) {
        let _ = data.publish_file_preview(FilePreviewState::Hidden);
    }
    match result {
        Err(error) => Err(error),
        Ok(selection) => {
            mouse_result?;
            Ok(selection)
        }
    }
}

#[derive(Default)]
struct CrosstermInput {
    mouse_capture_enabled: bool,
}

impl Input for CrosstermInput {
    fn set_mouse_capture(&mut self, enabled: bool) -> io::Result<()> {
        if enabled == self.mouse_capture_enabled {
            return Ok(());
        }
        let mut stdout = io::stdout();
        if enabled {
            execute!(stdout, EnableMouseCapture)?;
        } else {
            execute!(stdout, DisableMouseCapture)?;
        }
        self.mouse_capture_enabled = enabled;
        Ok(())
    }

    fn read_key(&mut self) -> io::Result<KeyEvent> {
        loop {
            if let Event::Key(key) = event::read()? {
                return Ok(key);
            }
        }
    }

    fn read_key_timeout(&mut self, timeout: Duration) -> io::Result<Option<KeyEvent>> {
        if !event::poll(timeout)? {
            return Ok(None);
        }
        loop {
            if let Event::Key(key) = event::read()? {
                return Ok(Some(key));
            }
            if !event::poll(Duration::ZERO)? {
                return Ok(None);
            }
        }
    }

    fn read_event_timeout(&mut self, timeout: Duration) -> io::Result<Option<InputEvent>> {
        if !event::poll(timeout)? {
            return Ok(None);
        }
        loop {
            let input = match event::read()? {
                Event::Key(key) => Some(InputEvent::Key(key)),
                Event::Mouse(mouse) => Some(InputEvent::Mouse(mouse)),
                Event::Resize(_, _) => Some(InputEvent::Resize),
                _ => None,
            };
            if input.is_some() || !event::poll(Duration::ZERO)? {
                return Ok(input);
            }
        }
    }
}
