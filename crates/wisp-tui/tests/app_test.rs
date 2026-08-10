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
    protocol::Selection,
};
use wisp_tui::{App, Command, DataSource, InitialView, Input, RightMode, run_with_terminal};

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
    directory_calls: Vec<PathBuf>,
    directory_error: Option<String>,
}

impl DataSource for FixtureData {
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
}

struct ScriptedInput(VecDeque<KeyEvent>);

impl Input for ScriptedInput {
    fn read_key(&mut self) -> io::Result<KeyEvent> {
        self.0
            .pop_front()
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "input exhausted"))
    }
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
