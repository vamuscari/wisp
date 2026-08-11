use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use thiserror::Error;
use wisp_core::{
    config::Openers,
    model::{DirectoryEntry, Project},
    navigation::{NavigationError, NavigationOutcome, Navigator, Screen},
    opencode::{OpenCodeSession, OpenCodeSnapshot, SessionDisplayState},
    protocol::{HostContext, HostWorkspaceContext, Selection},
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
pub enum InitialView {
    Projects,
    Windows,
    Sessions,
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
    fn directory(&mut self, path: &Path) -> Result<Vec<DirectoryEntry>, String>;
    fn refresh_projects(&mut self) -> Result<Vec<Project>, String>;
    fn refresh_directory(&mut self, path: &Path) -> Result<Vec<DirectoryEntry>, String>;
    fn sessions(&mut self, _path: &Path) -> Result<OpenCodeSnapshot, String> {
        Err("OpenCode integration is not configured".into())
    }
    fn refresh_sessions(&mut self, path: &Path) -> Result<OpenCodeSnapshot, String> {
        self.sessions(path)
    }
    fn session_updates_pending(&mut self) -> bool {
        false
    }
}

pub trait Input {
    fn read_key(&mut self) -> io::Result<KeyEvent>;
    fn read_key_timeout(&mut self, _timeout: Duration) -> io::Result<Option<KeyEvent>> {
        self.read_key().map(Some)
    }
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
    follow_symlinks: bool,
    context: HostContext,
    entries: Vec<DirectoryEntry>,
    opencode_command: Option<Vec<String>>,
    sessions: Vec<OpenCodeSession>,
    session_host_items: BTreeMap<String, String>,
    session_conflicts: BTreeSet<String>,
    project_query: String,
    detail_query: String,
    project_cursor: usize,
    detail_cursor: usize,
    focus: Focus,
    right_mode: RightMode,
    input_mode: InputMode,
    status: Option<String>,
    startup_command: Option<Command>,
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
            follow_symlinks,
            context: context.unwrap_or_default(),
            entries: Vec::new(),
            opencode_command,
            sessions: Vec::new(),
            session_host_items: BTreeMap::new(),
            session_conflicts: BTreeSet::new(),
            project_query: String::new(),
            detail_query: String::new(),
            project_cursor: 0,
            detail_cursor: 0,
            focus: Focus::Projects,
            right_mode: RightMode::Windows,
            input_mode: InputMode::Normal,
            status: None,
            startup_command: None,
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

    pub const fn detail_cursor(&self) -> usize {
        self.detail_cursor
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
        &self.detail_query
    }

    pub fn current_directory(&self) -> Option<&Path> {
        match self.navigator.screen() {
            Screen::Directory { path, .. } => Some(path),
            Screen::Projects => None,
        }
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

    pub fn replace_projects(&mut self, projects: Vec<Project>) {
        self.navigator = Navigator::new(projects, self.follow_symlinks);
        self.entries.clear();
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
        self.entries = entries;
        self.detail_query.clear();
        self.detail_cursor = 0;
        self.status = None;
    }

    pub fn load_sessions(&mut self, snapshot: OpenCodeSnapshot) {
        let selected_session_id = (self.right_mode == RightMode::Sessions)
            .then(|| self.visible_detail_items().get(self.detail_cursor).cloned())
            .flatten()
            .and_then(|item| match item.target {
                DetailTarget::OpenCodeSession(id) => Some(id),
                DetailTarget::HostItem(_) | DetailTarget::Entry(_) => None,
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
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('c') => Ok(Command::Cancel),
                KeyCode::Char('r') => Ok(self.refresh_command()),
                _ => Ok(Command::None),
            };
        }
        if self.input_mode == InputMode::Search {
            return self.handle_search_key(key.code);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Ok(Command::Cancel),
            KeyCode::Left | KeyCode::Char('h') => {
                self.focus = Focus::Projects;
                Ok(Command::None)
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.focus = Focus::Detail;
                Ok(Command::None)
            }
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Projects => Focus::Detail,
                    Focus::Detail => Focus::Projects,
                };
                Ok(Command::None)
            }
            KeyCode::Enter if self.focus == Focus::Projects => self.select_project(),
            KeyCode::Enter => self.select_detail(),
            KeyCode::Char('f') => self.show_files(),
            KeyCode::Char('s') => self.show_sessions(),
            KeyCode::Char('w') => {
                self.show_windows();
                Ok(Command::None)
            }
            KeyCode::Char('x') => self.close_selected_project(),
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Search;
                Ok(Command::None)
            }
            KeyCode::Backspace
                if self.focus == Focus::Detail && self.right_mode == RightMode::Files =>
            {
                self.back_file()
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
            KeyCode::Enter if self.focus == Focus::Projects => self.select_project(),
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
                if let Some(character) = character {
                    self.detail_query.push(character);
                } else {
                    self.detail_query.pop();
                }
                self.detail_cursor = self
                    .detail_cursor
                    .min(self.visible_detail_items().len().saturating_sub(1));
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
                self.detail_cursor = self.detail_cursor.saturating_sub(1);
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

    fn select_detail(&mut self) -> Result<Command, NavigationError> {
        let Some(item) = self.visible_detail_items().get(self.detail_cursor).cloned() else {
            return Ok(Command::None);
        };
        match item.target {
            DetailTarget::HostItem(id) => {
                let Some(target) = self.selected_target() else {
                    return Ok(Command::None);
                };
                match target {
                    ProjectTarget::Project(project) => {
                        command_for_outcome(self.navigator.select_host_item(&project.id, &id)?)
                    }
                    ProjectTarget::Workspace { name, .. } => {
                        Ok(Command::Finish(Selection::WorkspaceItem {
                            workspace: name,
                            id,
                        }))
                    }
                }
            }
            DetailTarget::Entry(entry) => {
                command_for_outcome(self.navigator.select_entry(&entry, &self.openers)?)
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

    fn show_files(&mut self) -> Result<Command, NavigationError> {
        if self.selected_workspace_name().is_some() {
            self.set_workspace_project_status();
            return Ok(Command::None);
        }
        self.focus = Focus::Detail;
        self.right_mode = RightMode::Files;
        self.entries.clear();
        self.sessions.clear();
        self.session_host_items.clear();
        self.session_conflicts.clear();
        self.detail_query.clear();
        self.detail_cursor = 0;
        let Some(project_id) = self.selected_project_id().map(str::to_owned) else {
            return Ok(Command::None);
        };
        command_for_outcome(self.navigator.browse_project(&project_id)?)
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
        self.entries.clear();
        self.sessions.clear();
        self.session_host_items.clear();
        self.session_conflicts.clear();
        self.detail_query.clear();
        self.detail_cursor = 0;
        self.status = None;
        Ok(self
            .selected_project_path()
            .map_or(Command::None, Command::LoadSessions))
    }

    fn show_windows(&mut self) {
        self.focus = Focus::Detail;
        self.right_mode = RightMode::Windows;
        self.navigator.show_projects();
        self.entries.clear();
        self.sessions.clear();
        self.session_host_items.clear();
        self.session_conflicts.clear();
        self.detail_query.clear();
        self.detail_cursor = 0;
        self.status = None;
        self.select_active_host_item();
    }

    fn back_file(&mut self) -> Result<Command, NavigationError> {
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
        if !item.status.is_open() {
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
                self.detail_cursor = self
                    .detail_cursor
                    .saturating_add(1)
                    .min(self.visible_detail_items().len().saturating_sub(1));
                Ok(Command::None)
            }
        }
    }

    fn project_changed(
        &mut self,
        previous: Option<ProjectTargetKey>,
    ) -> Result<Command, NavigationError> {
        let current = self.selected_target_key();
        if current == previous {
            return Ok(Command::None);
        }
        self.entries.clear();
        self.sessions.clear();
        self.session_host_items.clear();
        self.session_conflicts.clear();
        self.detail_query.clear();
        self.detail_cursor = 0;
        self.status = None;
        if self.selected_workspace_name().is_some() && self.right_mode != RightMode::Windows {
            self.right_mode = RightMode::Windows;
            self.set_workspace_project_status();
            self.select_active_host_item();
            return Ok(Command::None);
        }
        if self.right_mode == RightMode::Files {
            if let Some(project_id) = self.selected_project_id().map(str::to_owned) {
                return command_for_outcome(self.navigator.browse_project(&project_id)?);
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
            .is_some_and(|item| item.status == ProjectStatus::Current)
    }

    fn selected_project_is_open(&self) -> bool {
        self.visible_project_items()
            .get(self.project_cursor)
            .is_some_and(|item| item.status.is_open())
    }

    fn select_active_host_item(&mut self) {
        if let Some(index) = self
            .visible_detail_items()
            .iter()
            .position(|item| item.active)
        {
            self.detail_cursor = index;
        }
    }

    fn visible_project_items(&self) -> Vec<ProjectItem> {
        let mut items = self
            .navigator
            .projects()
            .iter()
            .enumerate()
            .map(|(order, project)| ProjectItem {
                target: ProjectTarget::Project(project.clone()),
                status: ProjectStatus::from_labels(self.context.labels(&project.id)),
                score: order,
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

    fn visible_detail_items(&self) -> Vec<DetailItem> {
        let Some(target) = self.selected_target() else {
            return Vec::new();
        };
        let mut items: Vec<DetailItem> = match self.right_mode {
            RightMode::Windows => match &target {
                ProjectTarget::Project(project) => self.context.items(&project.id),
                ProjectTarget::Workspace { context, .. } => &context.items,
            }
            .iter()
            .enumerate()
            .map(|(score, item)| DetailItem {
                label: item.label.clone(),
                detail: item.detail.clone(),
                active: item.active,
                score,
                target: DetailTarget::HostItem(item.id.clone()),
            })
            .collect(),
            RightMode::Files => self
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
                    active: false,
                    score,
                    target: DetailTarget::Entry(entry.clone()),
                })
                .collect(),
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
        if !self.detail_query.is_empty() {
            items.retain_mut(|item| {
                let candidate = item.detail.as_ref().map_or_else(
                    || item.label.clone(),
                    |detail| format!("{} {detail}", item.label),
                );
                let Some(score) = fuzzy_score(&self.detail_query, &candidate) else {
                    return false;
                };
                item.score = score;
                true
            });
            items.sort_by_key(|item| item.score);
        }
        items
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
    active: bool,
    score: usize,
    target: DetailTarget,
}

#[derive(Clone, Debug)]
enum DetailTarget {
    HostItem(String),
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

    const fn is_open(self) -> bool {
        matches!(self, Self::Current | Self::Open)
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

pub fn render(frame: &mut Frame, app: &App) {
    let [header, search, content, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(4),
        Constraint::Length(3),
    ])
    .areas(frame.area());
    let title = Line::from(vec![
        Span::styled(
            "WISP",
            Style::default()
                .fg(THEME.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  Projects"),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(Block::default().borders(Borders::ALL)),
        header,
    );
    let query = match app.focus {
        Focus::Projects => &app.project_query,
        Focus::Detail => &app.detail_query,
    };
    let search_text = if app.input_mode == InputMode::Search {
        format!("/ {query}")
    } else {
        "/ search  w windows  f files  s sessions  x close".into()
    };
    frame.render_widget(
        Paragraph::new(search_text)
            .style(Style::default().fg(if app.input_mode == InputMode::Search {
                THEME.input
            } else {
                THEME.muted
            }))
            .block(Block::default().borders(Borders::LEFT | Borders::RIGHT)),
        search,
    );

    let (projects_area, detail_area) = if content.width < 72 {
        let [projects_area, detail_area] =
            Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)])
                .areas(content);
        (projects_area, detail_area)
    } else {
        let [projects_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .areas(content);
        (projects_area, detail_area)
    };
    let project_rows = app
        .visible_project_items()
        .into_iter()
        .map(|item| {
            ListItem::new(Line::from(vec![
                Span::styled(item.status.icon(), Style::default().fg(item.status.color())),
                Span::raw(" "),
                Span::raw(item.target.display_name().to_string()),
            ]))
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
    let detail_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if app.focus == Focus::Detail {
            THEME.accent
        } else {
            THEME.muted
        }))
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
                RightMode::Files if !app.detail_query.is_empty() => "No matching files",
                RightMode::Files => "No files",
                RightMode::Sessions if !app.detail_query.is_empty() => "No matching sessions",
                RightMode::Sessions => "No OpenCode sessions",
            })
            .style(Style::default().fg(THEME.muted))
            .block(detail_block),
            detail_area,
        );
    } else {
        let mut detail_state = ListState::default().with_selected(Some(app.detail_cursor));
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

    let footer_text = app
        .status
        .as_deref()
        .unwrap_or("arrows/jk move | h/l/tab focus | enter select | q/esc cancel");
    let footer_style = if app.status.is_some() {
        Style::default().fg(THEME.error)
    } else {
        Style::default().fg(THEME.muted)
    };
    frame.render_widget(
        Paragraph::new(footer_text)
            .style(footer_style)
            .block(Block::default().borders(Borders::ALL)),
        footer,
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

    let result = run_with_terminal(&mut terminal, &mut app, data, &mut CrosstermInput);
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
    if let Some(Command::LoadSessions(path)) = app.take_startup_command() {
        match data.sessions(&path) {
            Ok(snapshot) => app.load_sessions(snapshot),
            Err(error) => app.set_status(error),
        }
    }
    loop {
        if app.right_mode == RightMode::Sessions && data.session_updates_pending() {
            if let Some(path) = app.selected_project_path() {
                match data.sessions(&path) {
                    Ok(snapshot) => app.load_sessions(snapshot),
                    Err(error) => app.set_status(error),
                }
            }
        }
        terminal.draw(|frame| render(frame, app))?;
        let Some(key) = input.read_key_timeout(Duration::from_millis(250))? else {
            continue;
        };
        let command = app.handle_key(key)?;
        match command {
            Command::None => {}
            Command::LoadDirectory(path) => match data.directory(&path) {
                Ok(entries) => app.load_directory(entries),
                Err(error) => app.set_status(error),
            },
            Command::LoadSessions(path) => match data.sessions(&path) {
                Ok(snapshot) => app.load_sessions(snapshot),
                Err(error) => app.set_status(error),
            },
            Command::RefreshProjects => match data.refresh_projects() {
                Ok(projects) => app.replace_projects(projects),
                Err(error) => app.set_status(error),
            },
            Command::RefreshDirectory(path) => match data.refresh_directory(&path) {
                Ok(entries) => app.load_directory(entries),
                Err(error) => app.set_status(error),
            },
            Command::RefreshSessions(path) => match data.refresh_sessions(&path) {
                Ok(snapshot) => app.load_sessions(snapshot),
                Err(error) => app.set_status(error),
            },
            Command::Finish(selection) => return Ok(Some(selection)),
            Command::Cancel => return Ok(None),
        }
    }
}

struct CrosstermInput;

impl Input for CrosstermInput {
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
}
