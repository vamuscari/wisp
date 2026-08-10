use std::{
    io,
    path::{Path, PathBuf},
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
    protocol::{HostContext, Selection},
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    None,
    LoadDirectory(PathBuf),
    RefreshProjects,
    RefreshDirectory(PathBuf),
    Finish(Selection),
    Cancel,
}

pub trait DataSource {
    fn directory(&mut self, path: &Path) -> Result<Vec<DirectoryEntry>, String>;
    fn refresh_projects(&mut self) -> Result<Vec<Project>, String>;
    fn refresh_directory(&mut self, path: &Path) -> Result<Vec<DirectoryEntry>, String>;
}

pub trait Input {
    fn read_key(&mut self) -> io::Result<KeyEvent>;
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
    project_query: String,
    detail_query: String,
    project_cursor: usize,
    detail_cursor: usize,
    focus: Focus,
    right_mode: RightMode,
    input_mode: InputMode,
    status: Option<String>,
}

impl App {
    pub fn new(
        projects: Vec<Project>,
        openers: Openers,
        follow_symlinks: bool,
        context: Option<HostContext>,
        initial_view: InitialView,
    ) -> Self {
        let mut app = Self {
            navigator: Navigator::new(projects, follow_symlinks),
            openers,
            follow_symlinks,
            context: context.unwrap_or_default(),
            entries: Vec::new(),
            project_query: String::new(),
            detail_query: String::new(),
            project_cursor: 0,
            detail_cursor: 0,
            focus: Focus::Projects,
            right_mode: RightMode::Windows,
            input_mode: InputMode::Normal,
            status: None,
        };
        app.select_current_project();
        if initial_view == InitialView::Windows {
            if app.selected_project_is_current() {
                app.focus = Focus::Detail;
                app.select_active_host_item();
            } else {
                app.status = Some("Current workspace is not a Wisp project".into());
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
        let project_id = self
            .visible_project_items()
            .get(self.project_cursor)
            .map(|item| item.project.id.clone())?;
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
            .map(|item| item.project.display_name)
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
                let previous = self.selected_project_id().map(str::to_owned);
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
                let previous = self.selected_project_id().map(str::to_owned);
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
        let Some(project_id) = self.selected_project_id() else {
            return Ok(Command::None);
        };
        command_for_outcome(self.navigator.select_project(project_id, &self.openers)?)
    }

    fn select_detail(&mut self) -> Result<Command, NavigationError> {
        let Some(item) = self.visible_detail_items().get(self.detail_cursor).cloned() else {
            return Ok(Command::None);
        };
        match item.target {
            DetailTarget::HostItem(id) => {
                let Some(project_id) = self.selected_project_id() else {
                    return Ok(Command::None);
                };
                command_for_outcome(self.navigator.select_host_item(project_id, &id)?)
            }
            DetailTarget::Entry(entry) => {
                command_for_outcome(self.navigator.select_entry(&entry, &self.openers)?)
            }
        }
    }

    fn show_files(&mut self) -> Result<Command, NavigationError> {
        self.focus = Focus::Detail;
        self.right_mode = RightMode::Files;
        self.entries.clear();
        self.detail_query.clear();
        self.detail_cursor = 0;
        let Some(project_id) = self.selected_project_id().map(str::to_owned) else {
            return Ok(Command::None);
        };
        command_for_outcome(self.navigator.browse_project(&project_id)?)
    }

    fn show_windows(&mut self) {
        self.focus = Focus::Detail;
        self.right_mode = RightMode::Windows;
        self.navigator.show_projects();
        self.entries.clear();
        self.detail_query.clear();
        self.detail_cursor = 0;
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
        command_for_outcome(self.navigator.close_project(&item.project.id)?)
    }

    fn move_down(&mut self) -> Result<Command, NavigationError> {
        match self.focus {
            Focus::Projects => {
                let previous = self.selected_project_id().map(str::to_owned);
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

    fn project_changed(&mut self, previous: Option<String>) -> Result<Command, NavigationError> {
        let current = self.selected_project_id().map(str::to_owned);
        if current == previous {
            return Ok(Command::None);
        }
        self.entries.clear();
        self.detail_query.clear();
        self.detail_cursor = 0;
        if self.right_mode == RightMode::Files {
            if let Some(project_id) = current {
                return command_for_outcome(self.navigator.browse_project(&project_id)?);
            }
        }
        Ok(Command::None)
    }

    fn refresh_command(&self) -> Command {
        if self.focus == Focus::Detail && self.right_mode == RightMode::Files {
            if let Some(path) = self.current_directory() {
                return Command::RefreshDirectory(path.to_path_buf());
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
                project: project.clone(),
                status: ProjectStatus::from_labels(self.context.labels(&project.id)),
                score: order,
            })
            .collect::<Vec<_>>();

        if !self.project_query.is_empty() {
            items.retain_mut(|item| {
                let Some(score) = fuzzy_score(&self.project_query, &item.project.display_name)
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
        let Some(project_id) = self.selected_project_id() else {
            return Vec::new();
        };
        let mut items: Vec<DetailItem> = match self.right_mode {
            RightMode::Windows => self
                .context
                .items(project_id)
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
    project: Project,
    status: ProjectStatus,
    score: usize,
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
        "/ search  w windows  f files  x close".into()
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
                Span::raw(item.project.display_name),
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
        });
    if detail_rows.is_empty() {
        frame.render_widget(
            Paragraph::new(match app.right_mode {
                RightMode::Windows if !app.selected_project_is_open() => "Project is not open",
                RightMode::Windows if !app.detail_query.is_empty() => "No matching windows",
                RightMode::Windows => "No windows",
                RightMode::Files if !app.detail_query.is_empty() => "No matching files",
                RightMode::Files => "No files",
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
    loop {
        terminal.draw(|frame| render(frame, app))?;
        let command = app.handle_key(input.read_key()?)?;
        match command {
            Command::None => {}
            Command::LoadDirectory(path) => match data.directory(&path) {
                Ok(entries) => app.load_directory(entries),
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
}
