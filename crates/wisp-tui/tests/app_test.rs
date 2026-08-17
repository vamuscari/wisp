use std::{
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use wisp_core::{
    config::Openers,
    model::{DirectoryEntry, EntryKind, Project},
    opencode::{OpenCodeSession, OpenCodeSnapshot, SessionActivity, SessionWaiting},
    protocol::Selection,
};
use wisp_tui::{
    ActiveProjectContext, App, Command, DataSource, GitSummary, InitialView, Input, RightMode,
    run_with_terminal,
};

fn projects() -> Vec<Project> {
    vec![
        Project {
            id: "api".into(),
            path: PathBuf::from("/repos/api"),
            group: "Repos".into(),
            name: "api".into(),
            display_name: "API Service".into(),
        },
        Project {
            id: "web".into(),
            path: PathBuf::from("/repos/web"),
            group: "Repos".into(),
            name: "web".into(),
            display_name: "Web Client".into(),
        },
    ]
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn file_selection_returns_the_resolved_opener() {
    let mut app = App::new(
        projects(),
        Openers {
            file: Some(vec!["nvim".into(), "{path}".into()]),
            project: None,
        },
        false,
        None,
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/api/README.md"),
        EntryKind::File,
    )]);

    let Command::Finish(selection) = app.handle_key(key(KeyCode::Enter)).unwrap() else {
        panic!("file enter should finish");
    };
    assert_eq!(
        selection,
        Selection::File {
            project: projects()[0].clone(),
            path: PathBuf::from("/repos/api/README.md"),
            opener: Some(vec!["nvim".into(), "/repos/api/README.md".into()]),
        }
    );
}

#[test]
fn ctrl_r_refreshes_projects_or_the_active_directory() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    assert_eq!(
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .unwrap(),
        Command::RefreshProjects
    );

    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    assert_eq!(
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .unwrap(),
        Command::RefreshDirectory(PathBuf::from("/repos/api"))
    );
}

#[test]
fn replacing_projects_resets_the_two_pane_view() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.replace_projects(vec![Project {
        id: "new".into(),
        path: PathBuf::from("/new"),
        group: "Home".into(),
        name: "new".into(),
        display_name: "New Project".into(),
    }]);

    assert_eq!(app.selected_project_id(), Some("new"));
    assert_eq!(app.visible_project_labels(), vec!["New Project"]);
    assert_eq!(app.right_mode(), RightMode::Windows);
    assert_eq!(app.current_directory(), None);
    assert!(app.project_query().is_empty());
    assert!(app.detail_query().is_empty());
}

#[derive(Default)]
struct FixtureData {
    active_project_git: Option<(String, GitSummary)>,
    active_project_git_delay: usize,
    directory_calls: Vec<PathBuf>,
    directory_error: Option<String>,
    session_calls: Vec<PathBuf>,
    session_snapshot: OpenCodeSnapshot,
}

impl DataSource for FixtureData {
    fn active_project_git_update(&mut self) -> Option<(String, GitSummary)> {
        if self.active_project_git_delay > 0 {
            self.active_project_git_delay -= 1;
            return None;
        }
        self.active_project_git.take()
    }

    fn directory(&mut self, path: &Path) -> Result<Vec<DirectoryEntry>, String> {
        self.directory_calls.push(path.to_path_buf());
        if let Some(error) = &self.directory_error {
            return Err(error.clone());
        }
        Ok(vec![DirectoryEntry::new(
            path.join("README.md"),
            EntryKind::File,
        )])
    }

    fn refresh_projects(&mut self) -> Result<Vec<Project>, String> {
        Ok(projects())
    }

    fn refresh_directory(&mut self, path: &Path) -> Result<Vec<DirectoryEntry>, String> {
        self.directory(path)
    }

    fn sessions(&mut self, path: &Path) -> Result<OpenCodeSnapshot, String> {
        self.session_calls.push(path.to_path_buf());
        Ok(self.session_snapshot.clone())
    }
}

struct ScriptedInput(VecDeque<KeyEvent>);

impl Input for ScriptedInput {
    fn read_key(&mut self) -> io::Result<KeyEvent> {
        self.0
            .pop_front()
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "input exhausted"))
    }
}

struct TimedInput(VecDeque<Option<KeyEvent>>);

impl Input for TimedInput {
    fn read_key(&mut self) -> io::Result<KeyEvent> {
        self.read_key_timeout(std::time::Duration::ZERO)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "input timed out"))
    }

    fn read_key_timeout(&mut self, _timeout: std::time::Duration) -> io::Result<Option<KeyEvent>> {
        self.0
            .pop_front()
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "input exhausted"))
    }
}

fn rendered_lines(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn terminal_loop_loads_files_lazily_and_returns_the_selection() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    let mut data = FixtureData::default();
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('f')),
        key(KeyCode::Enter),
    ]));
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let selection = run_with_terminal(&mut terminal, &mut app, &mut data, &mut input)
        .unwrap()
        .unwrap();

    assert_eq!(data.directory_calls, vec![PathBuf::from("/repos/api")]);
    assert_eq!(
        selection,
        Selection::File {
            project: projects()[0].clone(),
            path: PathBuf::from("/repos/api/README.md"),
            opener: None,
        }
    );
}

#[test]
fn terminal_loop_keeps_data_source_errors_visible_until_cancelled() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    let mut data = FixtureData {
        directory_error: Some("directory unavailable".into()),
        ..FixtureData::default()
    };
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('f')),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]));
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let selection = run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(selection, None);
    assert_eq!(app.status(), Some("directory unavailable"));
}

#[test]
fn sessions_initial_view_loads_before_input_and_returns_the_selected_session() {
    let mut app = App::new_with_opencode(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Sessions,
        vec!["opencode".into()],
    );
    let mut data = FixtureData {
        session_snapshot: OpenCodeSnapshot {
            sessions: vec![OpenCodeSession {
                id: "ses_123".into(),
                title: "Implement integration".into(),
                directory: PathBuf::from("/repos/api"),
                server_url: "http://127.0.0.1:4096".into(),
                agent: Some("build".into()),
                parent_id: None,
                updated_at: 20,
                activity: SessionActivity::Idle,
                waiting: SessionWaiting::default(),
            }],
            ..OpenCodeSnapshot::default()
        },
        ..FixtureData::default()
    };
    let mut input = ScriptedInput(VecDeque::from([key(KeyCode::Enter)]));
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let selection = run_with_terminal(&mut terminal, &mut app, &mut data, &mut input)
        .unwrap()
        .unwrap();

    assert_eq!(data.session_calls, vec![PathBuf::from("/repos/api")]);
    assert!(matches!(
        selection,
        Selection::OpenCodeSession { ref session_id, .. } if session_id == "ses_123"
    ));
}

#[test]
fn active_project_updates_the_sessions_initial_view_before_input() {
    let mut app = App::new_with_opencode(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Sessions,
        vec!["opencode".into()],
    );
    app.set_active_project_context(ActiveProjectContext {
        project_id: "web".into(),
        file: None,
        git: None,
    });
    let mut data = FixtureData::default();
    let mut input = ScriptedInput(VecDeque::from([KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )]));
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let selection = run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(selection, None);
    assert_eq!(data.session_calls, vec![PathBuf::from("/repos/web")]);
}

#[test]
fn terminal_loop_applies_a_delayed_active_project_git_update() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    app.set_active_project_context(ActiveProjectContext {
        project_id: "web".into(),
        file: None,
        git: None,
    });
    let mut data = FixtureData {
        active_project_git: Some((
            "web".into(),
            GitSummary {
                branch: "main".into(),
                dirty: true,
            },
        )),
        active_project_git_delay: 1,
        ..FixtureData::default()
    };
    let mut input = TimedInput(VecDeque::from([
        None,
        Some(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
    ]));
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let selection = run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");

    assert_eq!(selection, None);
    assert!(rendered.contains("◆ Web Client"));
    assert!(rendered.contains("main dirty"));
}
