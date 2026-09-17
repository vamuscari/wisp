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
    navigation::Screen,
    opencode::{OpenCodeSession, OpenCodeSnapshot, SessionActivity, SessionWaiting},
    protocol::{FileOpenTarget, HostContext, Selection},
};
use wisp_tui::{
    ActiveProjectContext, App, Command, DataSource, DirectoryRequest, DirectoryUpdate,
    FilePreviewContent, FilePreviewRequest, FilePreviewUpdate, GitSummary, InitialView, Input,
    RightMode, WindowPreviewContent, WindowPreviewRequest, WindowPreviewState, WindowPreviewUpdate,
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

fn file_target_context() -> HostContext {
    serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "windows": [{
                    "id": "17", "label": "editor", "active": true,
                    "panes": [{
                        "id": "71", "label": "editor", "active": true,
                        "nvim_views": [{
                            "window_id": "9", "path": "/repos/api/README.md", "active": true,
                            "width": 80, "height": 20, "bottomline": 20,
                            "view": { "lnum": 1, "col": 0, "coladd": 0, "curswant": 0, "topline": 1, "topfill": 0, "leftcol": 0, "skipcol": 0 }
                        }]
                    }]
                }]
            },
            "web": { "labels": ["new"] }
        },
        "workspaces": {}
    }))
    .unwrap()
}

fn app_with_selected_file() -> App {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(file_target_context()),
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/api/README.md"),
        EntryKind::File,
    )]);
    app
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
            open_target: FileOpenTarget::Window,
            reuse_existing: true,
            host_target: None,
        }
    );
}

#[test]
fn configured_file_open_target_changes_enter_selection() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    app.configure_file_open_target(FileOpenTarget::RightPane);
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/api/README.md"),
        EntryKind::File,
    )]);

    let Command::Finish(Selection::File {
        open_target,
        reuse_existing,
        host_target,
        ..
    }) = app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("file enter should finish");
    };
    assert_eq!(open_target, FileOpenTarget::RightPane);
    assert!(reuse_existing);
    assert_eq!(host_target, None);
}

#[test]
fn search_enter_uses_the_configured_default_and_live_target() {
    let mut app = app_with_selected_file();
    app.configure_file_open_target(FileOpenTarget::BottomPane);
    app.handle_key(key(KeyCode::Char('/'))).unwrap();

    let Command::Finish(Selection::File {
        open_target,
        reuse_existing,
        host_target,
        ..
    }) = app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("search enter should finish");
    };
    assert_eq!(open_target, FileOpenTarget::BottomPane);
    assert!(reuse_existing);
    assert_eq!(host_target.unwrap().pane_id, "71");
}

#[test]
fn ctrl_file_targets_force_fresh_normal_mode_selections() {
    for (character, expected) in [
        ('t', FileOpenTarget::Window),
        ('v', FileOpenTarget::RightPane),
        ('x', FileOpenTarget::BottomPane),
    ] {
        let mut app = app_with_selected_file();
        let Command::Finish(Selection::File {
            open_target,
            reuse_existing,
            host_target,
            ..
        }) = app
            .handle_key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::CONTROL,
            ))
            .unwrap()
        else {
            panic!("Ctrl-{character} should finish");
        };
        assert_eq!(open_target, expected);
        assert!(!reuse_existing);
        assert_eq!(host_target, None);
    }
}

#[test]
fn ctrl_file_targets_force_fresh_search_mode_selections() {
    for (character, expected) in [
        ('t', FileOpenTarget::Window),
        ('v', FileOpenTarget::RightPane),
        ('x', FileOpenTarget::BottomPane),
    ] {
        let mut app = app_with_selected_file();
        app.handle_key(key(KeyCode::Char('/'))).unwrap();
        let Command::Finish(Selection::File {
            open_target,
            reuse_existing,
            host_target,
            ..
        }) = app
            .handle_key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::CONTROL,
            ))
            .unwrap()
        else {
            panic!("Ctrl-{character} should finish from search");
        };
        assert_eq!(open_target, expected);
        assert!(!reuse_existing);
        assert_eq!(host_target, None);
    }
}

#[test]
fn ctrl_file_target_requires_a_file_in_files_detail_focus() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/api/src"),
        EntryKind::Directory,
    )]);
    let ctrl_t = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);

    assert_eq!(app.handle_key(ctrl_t).unwrap(), Command::None);
    assert_eq!(
        app.status(),
        Some("Select a file before choosing an open target")
    );
    app.clear_status();
    app.handle_key(key(KeyCode::Left)).unwrap();
    assert_eq!(app.handle_key(ctrl_t).unwrap(), Command::None);
    assert_eq!(
        app.status(),
        Some("Select a file before choosing an open target")
    );

    app.handle_key(key(KeyCode::Char('w'))).unwrap();
    app.clear_status();
    assert_eq!(app.handle_key(ctrl_t).unwrap(), Command::None);
    assert_eq!(app.status(), None);
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

#[test]
fn replacing_projects_restores_the_preferred_window_preview() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    app.configure_window_preview(true, true);
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    assert_eq!(app.auxiliary_pane(), wisp_tui::AuxiliaryPane::Hidden);

    app.replace_projects(projects());

    assert_eq!(app.right_mode(), RightMode::Windows);
    assert!(app.window_preview_visible());
    assert_eq!(app.auxiliary_pane(), wisp_tui::AuxiliaryPane::Preview);
}

#[derive(Default)]
struct FixtureData {
    project_git_updates: Vec<(String, GitSummary)>,
    project_git_update_delay: usize,
    directory_calls: Vec<PathBuf>,
    directory_results: VecDeque<Result<Vec<DirectoryEntry>, String>>,
    directory_error: Option<String>,
    session_calls: Vec<PathBuf>,
    session_snapshot: OpenCodeSnapshot,
    preview_requests: Vec<WindowPreviewRequest>,
    preview_cancellations: usize,
    preview_update: Option<WindowPreviewUpdate>,
    preview_update_delay: usize,
    file_preview_requests: Vec<FilePreviewRequest>,
    file_preview_updates: VecDeque<FilePreviewUpdate>,
    file_preview_updates_after_requests: usize,
}

impl DataSource for FixtureData {
    fn project_git_updates(&mut self) -> Vec<(String, GitSummary)> {
        if self.project_git_update_delay > 0 {
            self.project_git_update_delay -= 1;
            return Vec::new();
        }
        std::mem::take(&mut self.project_git_updates)
    }

    fn directory(&mut self, path: &Path) -> Result<Vec<DirectoryEntry>, String> {
        self.directory_calls.push(path.to_path_buf());
        if let Some(error) = &self.directory_error {
            return Err(error.clone());
        }
        if let Some(result) = self.directory_results.pop_front() {
            return result;
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

    fn request_window_preview(&mut self, request: WindowPreviewRequest) {
        self.preview_requests.push(request);
    }

    fn cancel_window_preview(&mut self) {
        self.preview_cancellations += 1;
    }

    fn window_preview_update(&mut self) -> Option<WindowPreviewUpdate> {
        if self.preview_update_delay > 0 {
            self.preview_update_delay -= 1;
            return None;
        }
        self.preview_update.take()
    }

    fn request_file_preview(&mut self, request: FilePreviewRequest) {
        self.file_preview_requests.push(request);
    }

    fn file_preview_update(&mut self) -> Option<FilePreviewUpdate> {
        if self.file_preview_requests.len() < self.file_preview_updates_after_requests.max(1) {
            return None;
        }
        self.file_preview_updates.pop_front()
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

struct MouseTrackingInput {
    events: VecDeque<KeyEvent>,
    mouse_capture: Vec<bool>,
}

impl Input for MouseTrackingInput {
    fn read_key(&mut self) -> io::Result<KeyEvent> {
        self.read_key_timeout(std::time::Duration::ZERO)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "input timed out"))
    }

    fn read_key_timeout(&mut self, _timeout: std::time::Duration) -> io::Result<Option<KeyEvent>> {
        self.events
            .pop_front()
            .map(Some)
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "input exhausted"))
    }

    fn set_mouse_capture(&mut self, enabled: bool) -> io::Result<()> {
        self.mouse_capture.push(enabled);
        Ok(())
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
            open_target: FileOpenTarget::Window,
            reuse_existing: true,
            host_target: None,
        }
    );
}

#[test]
fn terminal_loop_renders_file_preview_inside_the_picker() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    app.configure_file_preview(true, true);
    let mut data = FixtureData {
        file_preview_updates: VecDeque::from([FilePreviewUpdate {
            request_id: 1,
            path: PathBuf::from("/repos/api/README.md"),
            content: FilePreviewContent::Text {
                text: "# API\n\nInternal preview content".into(),
                truncated: false,
            },
        }]),
        ..FixtureData::default()
    };
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('f')),
        key(KeyCode::Char('q')),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    assert_eq!(
        run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap(),
        None
    );
    assert_eq!(
        data.file_preview_requests,
        vec![FilePreviewRequest {
            request_id: 1,
            path: PathBuf::from("/repos/api/README.md"),
        }]
    );
    let rendered = rendered_lines(&terminal).join("\n");
    assert!(rendered.contains("File Preview"));
    assert!(rendered.contains("Internal preview content"));
}

#[test]
fn stale_file_preview_updates_cannot_replace_the_new_target() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    app.configure_file_preview(true, true);
    let first = PathBuf::from("/repos/api/a.rs");
    let latest = PathBuf::from("/repos/api/b.rs");
    let mut data = FixtureData {
        directory_results: VecDeque::from([Ok(vec![
            DirectoryEntry::new(first.clone(), EntryKind::File),
            DirectoryEntry::new(latest.clone(), EntryKind::File),
        ])]),
        file_preview_updates: VecDeque::from([
            FilePreviewUpdate {
                request_id: 1,
                path: first.clone(),
                content: FilePreviewContent::Text {
                    text: "stale preview".into(),
                    truncated: false,
                },
            },
            FilePreviewUpdate {
                request_id: 2,
                path: latest.clone(),
                content: FilePreviewContent::Text {
                    text: "latest preview".into(),
                    truncated: false,
                },
            },
        ]),
        file_preview_updates_after_requests: 2,
        ..FixtureData::default()
    };
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('f')),
        key(KeyCode::Down),
        key(KeyCode::Null),
        key(KeyCode::Char('q')),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    assert_eq!(
        run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap(),
        None
    );
    assert_eq!(
        data.file_preview_requests,
        vec![
            FilePreviewRequest {
                request_id: 1,
                path: first,
            },
            FilePreviewRequest {
                request_id: 2,
                path: latest,
            },
        ]
    );
    let rendered = rendered_lines(&terminal).join("\n");
    assert!(rendered.contains("latest preview"));
    assert!(!rendered.contains("stale preview"));
}

#[test]
fn ctrl_r_requests_a_fresh_file_preview_for_the_same_path() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    app.configure_file_preview(true, true);
    let mut data = FixtureData::default();
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('f')),
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        key(KeyCode::Char('q')),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    assert_eq!(
        run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap(),
        None
    );
    assert_eq!(data.file_preview_requests.len(), 2);
    assert_eq!(data.file_preview_requests[0].request_id, 1);
    assert_eq!(data.file_preview_requests[1].request_id, 2);
    assert_eq!(
        data.file_preview_requests[0].path,
        data.file_preview_requests[1].path
    );
}

#[test]
fn terminal_loop_previews_only_the_highlighted_directory() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    let mut data = FixtureData {
        directory_results: VecDeque::from([
            Ok(vec![DirectoryEntry::new(
                PathBuf::from("/repos/api/src"),
                EntryKind::Directory,
            )]),
            Ok(vec![DirectoryEntry::new(
                PathBuf::from("/repos/api/src/lib.rs"),
                EntryKind::File,
            )]),
        ]),
        ..FixtureData::default()
    };
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('f')),
        key(KeyCode::Char('q')),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let selection = run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(selection, None);
    assert_eq!(
        data.directory_calls,
        vec![PathBuf::from("/repos/api"), PathBuf::from("/repos/api/src")]
    );
    assert_eq!(app.current_directory(), Some(Path::new("/repos/api")));
    assert_eq!(app.visible_detail_labels(), vec!["src/"]);
}

struct DeferredDirectoryData {
    requests: Vec<DirectoryRequest>,
    stale_request: Option<DirectoryRequest>,
    updates: VecDeque<DirectoryUpdate>,
}

impl DataSource for DeferredDirectoryData {
    fn directory(&mut self, _path: &Path) -> Result<Vec<DirectoryEntry>, String> {
        unreachable!("directory requests should use the asynchronous contract")
    }

    fn refresh_projects(&mut self) -> Result<Vec<Project>, String> {
        Ok(projects())
    }

    fn refresh_directory(&mut self, _path: &Path) -> Result<Vec<DirectoryEntry>, String> {
        unreachable!("directory requests should use the asynchronous contract")
    }

    fn request_directory(&mut self, request: DirectoryRequest) -> Option<DirectoryUpdate> {
        self.requests.push(request.clone());
        match request.path.to_string_lossy().as_ref() {
            "/repos/api" => Some(DirectoryUpdate {
                request_id: request.request_id,
                path: request.path,
                result: Ok(vec![
                    DirectoryEntry::new(PathBuf::from("/repos/api/a"), EntryKind::Directory),
                    DirectoryEntry::new(PathBuf::from("/repos/api/b"), EntryKind::Directory),
                ]),
            }),
            "/repos/api/a" => {
                self.stale_request = Some(request);
                None
            }
            "/repos/api/b" => {
                let stale = self.stale_request.take().unwrap();
                self.updates.push_back(DirectoryUpdate {
                    request_id: stale.request_id,
                    path: stale.path.clone(),
                    result: Ok(vec![DirectoryEntry::new(
                        stale.path.join("a.txt"),
                        EntryKind::File,
                    )]),
                });
                self.updates.push_back(DirectoryUpdate {
                    request_id: request.request_id,
                    path: request.path.clone(),
                    result: Ok(vec![DirectoryEntry::new(
                        request.path.join("b.txt"),
                        EntryKind::File,
                    )]),
                });
                None
            }
            path => panic!("unexpected directory request {path}"),
        }
    }

    fn directory_update(&mut self) -> Option<DirectoryUpdate> {
        self.updates.pop_front()
    }
}

#[test]
fn stale_highlighted_directory_results_cannot_replace_the_latest_column() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    let mut data = DeferredDirectoryData {
        requests: Vec::new(),
        stale_request: None,
        updates: VecDeque::new(),
    };
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('f')),
        key(KeyCode::Down),
        key(KeyCode::Enter),
        key(KeyCode::Char('q')),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let selection = run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(selection, None);
    assert_eq!(app.current_directory(), Some(Path::new("/repos/api/b")));
    assert_eq!(app.visible_detail_labels(), vec!["b.txt"]);
    assert_eq!(
        data.requests
            .iter()
            .map(|request| request.path.as_path())
            .collect::<Vec<_>>(),
        vec![
            Path::new("/repos/api"),
            Path::new("/repos/api/a"),
            Path::new("/repos/api/b")
        ]
    );
}

#[test]
fn changing_highlight_cancels_an_unfinished_directory_descent() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    let mut data = DeferredDirectoryData {
        requests: Vec::new(),
        stale_request: None,
        updates: VecDeque::new(),
    };
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('f')),
        key(KeyCode::Enter),
        key(KeyCode::Down),
        key(KeyCode::Enter),
        key(KeyCode::Backspace),
        key(KeyCode::Char('q')),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let selection = run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(selection, None);
    assert_eq!(app.current_directory(), Some(Path::new("/repos/api")));
    assert_eq!(app.visible_detail_labels(), vec!["a/", "b/"]);
}

#[test]
fn leaving_files_cancels_an_unfinished_directory_descent() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    let mut data = DeferredDirectoryData {
        requests: Vec::new(),
        stale_request: None,
        updates: VecDeque::new(),
    };
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('f')),
        key(KeyCode::Enter),
        key(KeyCode::Left),
        key(KeyCode::Char('q')),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(app.focus(), wisp_tui::Focus::Detail);
    assert!(matches!(
        app.screen(),
        Screen::Directory { path, ancestors, .. }
            if path == Path::new("/repos/api") && ancestors.is_empty()
    ));
}

#[test]
fn refreshing_files_cancels_an_unfinished_directory_descent() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    let mut data = DeferredDirectoryData {
        requests: Vec::new(),
        stale_request: None,
        updates: VecDeque::new(),
    };
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('f')),
        key(KeyCode::Enter),
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        key(KeyCode::Char('q')),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert!(matches!(
        app.screen(),
        Screen::Directory { path, ancestors, .. }
            if path == Path::new("/repos/api") && ancestors.is_empty()
    ));
}

#[test]
fn failed_directory_descent_restores_the_parent_navigation_state() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    let root_entries = vec![
        DirectoryEntry::new(PathBuf::from("/repos/api/README.md"), EntryKind::File),
        DirectoryEntry::new(PathBuf::from("/repos/api/src"), EntryKind::Directory),
    ];
    let mut data = FixtureData {
        directory_results: VecDeque::from([
            Ok(root_entries.clone()),
            Err("preview failed".into()),
            Err("descent failed".into()),
            Ok(root_entries),
        ]),
        ..FixtureData::default()
    };
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('f')),
        key(KeyCode::Down),
        key(KeyCode::Enter),
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        key(KeyCode::Char('q')),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let selection = run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(selection, None);
    assert_eq!(app.current_directory(), Some(Path::new("/repos/api")));
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
        project_git_updates: vec![(
            "web".into(),
            GitSummary {
                branch: "main".into(),
                dirty: true,
                ..GitSummary::default()
            },
        )],
        project_git_update_delay: 1,
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
    assert!(rendered.contains("main ✗"));
}

#[test]
fn terminal_loop_applies_every_ready_project_git_update_before_input() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    let mut data = FixtureData {
        project_git_updates: vec![
            (
                "api".into(),
                GitSummary {
                    branch: "api-main".into(),
                    ..GitSummary::default()
                },
            ),
            (
                "web".into(),
                GitSummary {
                    branch: "web-main".into(),
                    ..GitSummary::default()
                },
            ),
        ],
        ..FixtureData::default()
    };
    let mut input = ScriptedInput(VecDeque::from([KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");

    assert!(rendered.contains("api-main"));
    assert!(rendered.contains("web-main"));
}

#[test]
fn terminal_loop_requests_and_applies_the_selected_window_preview() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "windows": [{
                    "id": "17", "label": "editor", "active": true,
                    "panes": [{ "id": "42", "label": "editor", "active": true }]
                }]
            },
            "web": { "labels": ["new"] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Windows,
    );
    app.configure_window_preview(true, true);
    let mut data = FixtureData {
        preview_update: Some(WindowPreviewUpdate {
            request_id: 1,
            pane_id: "42".into(),
            content: WindowPreviewContent::Text("server ready".into()),
        }),
        ..FixtureData::default()
    };
    let mut input = ScriptedInput(VecDeque::from([KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(
        data.preview_requests,
        vec![WindowPreviewRequest {
            request_id: 1,
            pane_id: "42".into(),
        }]
    );
    assert_eq!(
        app.window_preview_state(),
        &WindowPreviewState::Ready("server ready".into())
    );
    assert!(
        rendered_lines(&terminal)
            .join("\n")
            .contains("server ready")
    );
}

#[test]
fn on_demand_preview_waits_for_p_before_requesting_text() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "windows": [{
                    "id": "17", "label": "editor", "active": true,
                    "panes": [{ "id": "42", "label": "editor", "active": true }]
                }]
            },
            "web": { "labels": ["new"] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Windows,
    );
    app.configure_window_preview(true, false);
    let mut data = FixtureData::default();
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('p')),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(
        data.preview_requests,
        vec![WindowPreviewRequest {
            request_id: 1,
            pane_id: "42".into(),
        }]
    );
}

#[test]
fn commands_cancel_and_restore_the_visible_preview() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "windows": [{
                    "id": "17", "label": "editor", "active": true,
                    "panes": [{ "id": "42", "label": "editor", "active": true }]
                }]
            },
            "web": { "labels": ["new"] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Windows,
    );
    app.configure_window_preview(true, true);
    let mut data = FixtureData::default();
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('?')),
        key(KeyCode::Char('?')),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(data.preview_requests.len(), 2);
    assert_eq!(data.preview_cancellations, 1);
}

#[test]
fn mouse_capture_tracks_visible_preview_state() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "windows": [{
                    "id": "17", "label": "editor", "active": true,
                    "panes": [{ "id": "42", "label": "editor", "active": true }]
                }]
            },
            "web": { "labels": ["new"] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Windows,
    );
    app.configure_window_preview(true, false);
    let mut data = FixtureData::default();
    let mut input = MouseTrackingInput {
        events: VecDeque::from([
            key(KeyCode::Char('p')),
            key(KeyCode::Char('?')),
            key(KeyCode::Char('?')),
            key(KeyCode::Char('p')),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ]),
        mouse_capture: Vec::new(),
    };
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(input.mouse_capture, vec![true, false, true, false]);
}

#[test]
fn ctrl_r_requests_a_fresh_preview_for_the_same_window() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "windows": [{
                    "id": "17", "label": "editor", "active": true,
                    "panes": [{ "id": "42", "label": "editor", "active": true }]
                }]
            },
            "web": { "labels": ["new"] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Windows,
    );
    app.configure_window_preview(true, true);
    let mut data = FixtureData::default();
    let mut input = ScriptedInput(VecDeque::from([
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(data.preview_requests.len(), 2);
    assert_eq!(data.preview_requests[0].pane_id, "42");
    assert_eq!(data.preview_requests[1].pane_id, "42");
    assert!(data.preview_requests[1].request_id > data.preview_requests[0].request_id);
}

#[test]
fn ctrl_r_preserves_a_non_default_window_preview_target() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "windows": [
                    { "id": "17", "label": "first", "panes": [{ "id": "41", "label": "first" }] },
                    { "id": "18", "label": "second", "active": true, "panes": [{ "id": "42", "label": "second", "active": true }] },
                    { "id": "19", "label": "third", "panes": [{ "id": "43", "label": "third" }] }
                ]
            },
            "web": { "labels": ["new"] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Windows,
    );
    app.configure_window_preview(true, true);
    let mut data = FixtureData::default();
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Down),
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(
        data.preview_requests
            .iter()
            .map(|request| request.pane_id.as_str())
            .collect::<Vec<_>>(),
        vec!["42", "43", "43"]
    );
}

#[test]
fn ctrl_r_preserves_exact_pane_focus() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "windows": [{
                    "id": "17", "label": "editor", "active": true,
                    "panes": [
                        { "id": "41", "label": "editor", "active": true },
                        { "id": "42", "label": "logs" }
                    ]
                }]
            },
            "web": { "labels": ["new"] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Windows,
    );
    app.configure_window_preview(true, true);
    let mut data = FixtureData::default();
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Enter),
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        key(KeyCode::Enter),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let selection = run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    let Some(Selection::HostPane {
        window_id, pane_id, ..
    }) = selection
    else {
        panic!("refresh should retain Pane focus for the next Enter");
    };
    assert_eq!(window_id, "17");
    assert_eq!(pane_id, "41");
}

#[test]
fn leaving_windows_cancels_a_pending_preview_request() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "windows": [{
                    "id": "17", "label": "editor", "active": true,
                    "panes": [{ "id": "42", "label": "editor", "active": true }]
                }]
            },
            "web": { "labels": ["new"] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Windows,
    );
    app.configure_window_preview(true, true);
    let mut data = FixtureData::default();
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Char('f')),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(data.preview_cancellations, 1);
}

#[test]
fn stale_window_preview_updates_cannot_replace_the_new_target() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "windows": [
                    { "id": "17", "label": "first", "panes": [{ "id": "41", "label": "first" }] },
                    { "id": "18", "label": "second", "active": true, "panes": [{ "id": "42", "label": "second", "active": true }] }
                ]
            },
            "web": { "labels": ["new"] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Windows,
    );
    app.configure_window_preview(true, true);
    let mut data = FixtureData {
        preview_update: Some(WindowPreviewUpdate {
            request_id: 1,
            pane_id: "42".into(),
            content: WindowPreviewContent::Text("stale output".into()),
        }),
        preview_update_delay: 1,
        ..FixtureData::default()
    };
    let mut input = ScriptedInput(VecDeque::from([
        key(KeyCode::Up),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ]));
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();

    assert_eq!(data.preview_requests[0].pane_id, "42");
    assert_eq!(data.preview_requests[1].pane_id, "41");
    assert_eq!(app.window_preview_state(), &WindowPreviewState::Loading);
}

#[test]
fn narrow_window_preview_stacks_below_the_window_list() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "windows": [{
                    "id": "17", "label": "editor", "active": true,
                    "panes": [{ "id": "42", "label": "editor", "active": true }]
                }]
            },
            "web": { "labels": ["new"] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Windows,
    );
    app.configure_window_preview(true, true);
    let mut data = FixtureData {
        preview_update: Some(WindowPreviewUpdate {
            request_id: 1,
            pane_id: "42".into(),
            content: WindowPreviewContent::Unavailable,
        }),
        ..FixtureData::default()
    };
    let mut input = ScriptedInput(VecDeque::from([KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )]));
    let backend = TestBackend::new(60, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    run_with_terminal(&mut terminal, &mut app, &mut data, &mut input).unwrap();
    let lines = rendered_lines(&terminal);
    let windows = lines
        .iter()
        .position(|line| line.contains("Windows"))
        .unwrap();
    let preview = lines
        .iter()
        .position(|line| line.contains("Preview"))
        .unwrap();

    assert!(preview > windows);
    assert!(lines.join("\n").contains("Preview unavailable"));
}
