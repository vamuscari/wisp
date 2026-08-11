use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, style::Color};
use wisp_core::{
    config::Openers,
    model::{DirectoryEntry, EntryKind, Project},
    opencode::{OpenCodeSession, OpenCodeSnapshot, SessionActivity, SessionWaiting},
    protocol::{HostContext, Selection},
};
use wisp_tui::{App, Command, Focus, InitialView, InputMode, RightMode, render};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

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
        Project {
            id: "docs".into(),
            path: PathBuf::from("/repos/docs"),
            group: "Repos".into(),
            name: "docs".into(),
            display_name: "Documentation".into(),
        },
    ]
}

fn context() -> HostContext {
    serde_json::from_value(serde_json::json!({
        "protocol_version": 3,
        "projects": {
            "api": {
                "labels": ["new"],
                "items": [{ "id": "11", "label": "api-shell" }]
            },
            "web": {
                "labels": ["open"],
                "items": [{ "id": "12", "label": "web-server" }]
            },
            "docs": {
                "labels": ["current", "open"],
                "items": [
                    { "id": "17", "label": "editor" },
                    { "id": "18", "label": "docs-shell", "detail": "docs/", "active": true }
                ]
            }
        },
        "workspaces": {}
    }))
    .unwrap()
}

fn host_workspace_context() -> HostContext {
    serde_json::from_value(serde_json::json!({
        "protocol_version": 3,
        "projects": {
            "api": { "labels": ["open"] },
            "web": { "labels": ["new"] },
            "docs": { "labels": ["new"] }
        },
        "workspaces": {
            "default": {
                "current": true,
                "items": [
                    { "id": "29", "label": "shell", "active": true }
                ]
            }
        }
    }))
    .unwrap()
}

fn session(
    id: &str,
    title: &str,
    parent_id: Option<&str>,
    activity: SessionActivity,
    waiting: SessionWaiting,
) -> OpenCodeSession {
    OpenCodeSession {
        id: id.into(),
        title: title.into(),
        directory: PathBuf::from("/repos/docs"),
        server_url: "http://127.0.0.1:4096".into(),
        agent: Some(if id == "ses_urgent" { "plan" } else { "build" }.into()),
        parent_id: parent_id.map(str::to_owned),
        updated_at: 10,
        activity,
        waiting,
    }
}

#[test]
fn sessions_command_loads_the_selected_project_and_groups_children() {
    let mut app = App::new_with_opencode(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
        vec!["opencode".into()],
    );

    assert_eq!(
        app.handle_key(key(KeyCode::Char('s'))).unwrap(),
        Command::LoadSessions(PathBuf::from("/repos/docs"))
    );
    assert_eq!(app.right_mode(), RightMode::Sessions);
    assert_eq!(app.focus(), Focus::Detail);
    app.load_sessions(OpenCodeSnapshot {
        sessions: vec![
            session(
                "ses_root",
                "Root task",
                None,
                SessionActivity::Idle,
                SessionWaiting::default(),
            ),
            session(
                "ses_child",
                "Child task",
                Some("ses_root"),
                SessionActivity::Running,
                SessionWaiting::default(),
            ),
            session(
                "ses_urgent",
                "Needs input",
                None,
                SessionActivity::Running,
                SessionWaiting {
                    permissions: 0,
                    questions: 1,
                },
            ),
        ],
        ..OpenCodeSnapshot::default()
    });

    assert_eq!(
        app.visible_detail_labels(),
        vec!["Needs input", "Root task", "  Child task"]
    );
}

#[test]
fn selecting_a_session_uses_the_exact_host_mapping_and_attach_argv() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 3,
        "projects": {
            "api": { "labels": ["new"] },
            "web": { "labels": ["open"] },
            "docs": {
                "labels": ["current", "open"],
                "session_items": { "ses_urgent": "18" }
            }
        },
        "workspaces": {}
    }))
    .unwrap();
    let mut app = App::new_with_opencode(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Projects,
        vec!["opencode".into()],
    );
    app.handle_key(key(KeyCode::Char('s'))).unwrap();
    app.load_sessions(OpenCodeSnapshot {
        sessions: vec![session(
            "ses_urgent",
            "Needs input",
            None,
            SessionActivity::Running,
            SessionWaiting {
                permissions: 1,
                questions: 0,
            },
        )],
        ..OpenCodeSnapshot::default()
    });

    let Command::Finish(Selection::OpenCodeSession {
        project,
        session_id,
        opener,
        host_item_id,
    }) = app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("session enter should finish with an OpenCode selection")
    };
    assert_eq!(project.id, "docs");
    assert_eq!(session_id, "ses_urgent");
    assert_eq!(host_item_id.as_deref(), Some("18"));
    assert_eq!(
        opener,
        vec![
            "opencode",
            "attach",
            "http://127.0.0.1:4096",
            "--dir",
            "/repos/docs",
            "--session",
            "ses_urgent",
        ]
    );
}

#[test]
fn sessions_waiting_on_questions_sort_before_permissions() {
    let mut app = App::new_with_opencode(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
        vec!["opencode".into()],
    );
    app.handle_key(key(KeyCode::Char('s'))).unwrap();
    let mut permission = session(
        "ses_permission",
        "Permission",
        None,
        SessionActivity::Running,
        SessionWaiting {
            permissions: 1,
            questions: 0,
        },
    );
    permission.updated_at = 100;
    let question = session(
        "ses_question",
        "Question",
        None,
        SessionActivity::Running,
        SessionWaiting {
            permissions: 0,
            questions: 1,
        },
    );
    app.load_sessions(OpenCodeSnapshot {
        sessions: vec![permission, question],
        ..OpenCodeSnapshot::default()
    });

    assert_eq!(app.visible_detail_labels(), vec!["Question", "Permission"]);
}

#[test]
fn session_refresh_preserves_the_selected_session_when_status_reorders_rows() {
    let mut app = App::new_with_opencode(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
        vec!["opencode".into()],
    );
    app.handle_key(key(KeyCode::Char('s'))).unwrap();
    app.load_sessions(OpenCodeSnapshot {
        sessions: vec![
            session(
                "ses_first",
                "First",
                None,
                SessionActivity::Running,
                SessionWaiting {
                    permissions: 0,
                    questions: 1,
                },
            ),
            session(
                "ses_selected",
                "Selected",
                None,
                SessionActivity::Idle,
                SessionWaiting::default(),
            ),
        ],
        ..OpenCodeSnapshot::default()
    });
    app.handle_key(key(KeyCode::Down)).unwrap();

    app.load_sessions(OpenCodeSnapshot {
        sessions: vec![
            session(
                "ses_first",
                "First",
                None,
                SessionActivity::Idle,
                SessionWaiting::default(),
            ),
            session(
                "ses_selected",
                "Selected",
                None,
                SessionActivity::Running,
                SessionWaiting {
                    permissions: 0,
                    questions: 1,
                },
            ),
        ],
        ..OpenCodeSnapshot::default()
    });

    let Command::Finish(Selection::OpenCodeSession { session_id, .. }) =
        app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("session enter should finish with an OpenCode selection");
    };
    assert_eq!(session_id, "ses_selected");
}

#[test]
fn windows_initial_view_focuses_the_current_projects_active_window() {
    let app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );

    assert_eq!(app.focus(), Focus::Detail);
    assert_eq!(app.selected_project_id(), Some("docs"));
    assert_eq!(app.detail_cursor(), 1);
}

#[test]
fn windows_initial_view_focuses_a_current_host_workspace() {
    let app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(host_workspace_context()),
        InitialView::Windows,
    );

    assert_eq!(app.focus(), Focus::Detail);
    assert_eq!(app.selected_project_id(), None);
    assert_eq!(
        app.visible_project_labels(),
        vec!["default", "API Service", "Web Client", "Documentation"]
    );
    assert_eq!(app.visible_detail_labels(), vec!["shell"]);
    assert_eq!(app.detail_cursor(), 0);
}

#[test]
fn windows_initial_view_falls_back_when_the_workspace_is_unmanaged() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 3,
        "projects": {
            "api": { "labels": ["open"], "items": [] },
            "web": { "labels": ["new"], "items": [] },
            "docs": { "labels": ["new"], "items": [] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Windows,
    );

    assert_eq!(app.focus(), Focus::Projects);
    assert_eq!(
        app.status(),
        Some("Current workspace is not a Wisp project")
    );
}

#[test]
fn arrows_and_tab_change_two_pane_focus() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );

    app.handle_key(key(KeyCode::Right)).unwrap();
    assert_eq!(app.focus(), Focus::Detail);
    app.handle_key(key(KeyCode::Left)).unwrap();
    assert_eq!(app.focus(), Focus::Projects);
    app.handle_key(key(KeyCode::Tab)).unwrap();
    assert_eq!(app.focus(), Focus::Detail);
    app.handle_key(key(KeyCode::Tab)).unwrap();
    assert_eq!(app.focus(), Focus::Projects);
}

#[test]
fn moving_projects_immediately_scopes_the_windows_pane() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );

    app.handle_key(key(KeyCode::Down)).unwrap();
    assert_eq!(app.selected_project_id(), Some("web"));
    assert_eq!(app.visible_detail_labels(), vec!["web-server"]);

    app.handle_key(key(KeyCode::Char('j'))).unwrap();
    assert_eq!(app.selected_project_id(), Some("api"));
    assert_eq!(app.visible_detail_labels(), vec!["api-shell"]);

    app.handle_key(key(KeyCode::Char('k'))).unwrap();
    assert_eq!(app.selected_project_id(), Some("web"));
}

#[test]
fn enter_on_the_project_pane_returns_the_selected_project() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );

    let Command::Finish(Selection::Project { project, opener }) =
        app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("project enter should finish with a project selection");
    };
    assert_eq!(project.id, "docs");
    assert_eq!(opener, None);
}

#[test]
fn enter_on_a_host_workspace_returns_the_exact_workspace() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(host_workspace_context()),
        InitialView::Projects,
    );

    let Command::Finish(Selection::Workspace { workspace }) =
        app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("workspace enter should finish with a workspace selection");
    };
    assert_eq!(workspace, "default");
}

#[test]
fn enter_on_the_windows_pane_returns_the_selected_host_item() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );

    let Command::Finish(Selection::HostItem { project, id }) =
        app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("window enter should finish with a host-item selection");
    };
    assert_eq!(project.id, "docs");
    assert_eq!(id, "18");
}

#[test]
fn enter_on_a_host_workspaces_window_returns_the_exact_target() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(host_workspace_context()),
        InitialView::Windows,
    );

    let Command::Finish(Selection::WorkspaceItem { workspace, id }) =
        app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("workspace window enter should finish with an exact host target");
    };
    assert_eq!(workspace, "default");
    assert_eq!(id, "29");
}

#[test]
fn files_are_unavailable_for_a_host_workspace() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(host_workspace_context()),
        InitialView::Projects,
    );

    assert_eq!(
        app.handle_key(key(KeyCode::Char('f'))).unwrap(),
        Command::None
    );
    assert_eq!(app.right_mode(), RightMode::Windows);
    assert_eq!(
        app.status(),
        Some("Workspace default is not a Wisp project")
    );
}

#[test]
fn opencode_sessions_are_unavailable_for_a_host_workspace() {
    let mut app = App::new_with_opencode(
        projects(),
        Openers::default(),
        false,
        Some(host_workspace_context()),
        InitialView::Projects,
        vec!["opencode".into()],
    );

    assert_eq!(
        app.handle_key(key(KeyCode::Char('s'))).unwrap(),
        Command::None
    );
    assert_eq!(app.right_mode(), RightMode::Windows);
    assert_eq!(
        app.status(),
        Some("Workspace default is not a Wisp project")
    );
}

#[test]
fn sessions_initial_view_reports_a_current_host_workspace() {
    let app = App::new_with_opencode(
        projects(),
        Openers::default(),
        false,
        Some(host_workspace_context()),
        InitialView::Sessions,
        vec!["opencode".into()],
    );

    assert_eq!(app.focus(), Focus::Projects);
    assert_eq!(app.right_mode(), RightMode::Windows);
    assert_eq!(
        app.status(),
        Some("Workspace default is not a Wisp project")
    );
}

#[test]
fn files_command_focuses_files_and_requests_the_project_root() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );

    assert_eq!(
        app.handle_key(key(KeyCode::Char('f'))).unwrap(),
        Command::LoadDirectory(PathBuf::from("/repos/docs"))
    );
    assert_eq!(app.focus(), Focus::Detail);
    assert_eq!(app.right_mode(), RightMode::Files);
    assert_eq!(
        app.current_directory(),
        Some(PathBuf::from("/repos/docs").as_path())
    );
}

#[test]
fn entering_a_directory_loads_it_lazily() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![
        DirectoryEntry::new(PathBuf::from("/repos/docs/src"), EntryKind::Directory),
        DirectoryEntry::new(PathBuf::from("/repos/docs/README.md"), EntryKind::File),
    ]);

    assert_eq!(app.visible_detail_labels(), vec!["README.md", "src/"]);
    app.handle_key(key(KeyCode::Down)).unwrap();
    assert_eq!(
        app.handle_key(key(KeyCode::Enter)).unwrap(),
        Command::LoadDirectory(PathBuf::from("/repos/docs/src"))
    );
    assert_eq!(
        app.current_directory(),
        Some(PathBuf::from("/repos/docs/src").as_path())
    );
}

#[test]
fn backspace_moves_to_the_parent_then_focuses_projects_at_the_root() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/docs/src"),
        EntryKind::Directory,
    )]);
    app.handle_key(key(KeyCode::Enter)).unwrap();
    app.load_directory(Vec::new());

    assert_eq!(
        app.handle_key(key(KeyCode::Backspace)).unwrap(),
        Command::LoadDirectory(PathBuf::from("/repos/docs"))
    );
    assert_eq!(
        app.current_directory(),
        Some(PathBuf::from("/repos/docs").as_path())
    );

    app.load_directory(Vec::new());
    assert_eq!(
        app.handle_key(key(KeyCode::Backspace)).unwrap(),
        Command::None
    );
    assert_eq!(app.focus(), Focus::Projects);
    assert_eq!(app.current_directory(), None);
}

#[test]
fn moving_projects_in_files_mode_requests_the_new_project_root() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.handle_key(key(KeyCode::Left)).unwrap();

    assert_eq!(
        app.handle_key(key(KeyCode::Down)).unwrap(),
        Command::LoadDirectory(PathBuf::from("/repos/web"))
    );
    assert_eq!(app.selected_project_id(), Some("web"));
    assert_eq!(
        app.current_directory(),
        Some(PathBuf::from("/repos/web").as_path())
    );
}

#[test]
fn windows_command_returns_to_the_selected_projects_host_items() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/docs/README.md"),
        EntryKind::File,
    )]);

    assert_eq!(
        app.handle_key(key(KeyCode::Char('w'))).unwrap(),
        Command::None
    );
    assert_eq!(app.focus(), Focus::Detail);
    assert_eq!(app.right_mode(), RightMode::Windows);
    assert_eq!(app.visible_detail_labels(), vec!["editor", "docs-shell"]);
    assert_eq!(app.current_directory(), None);
}

#[test]
fn windows_command_selects_the_active_host_item() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('f'))).unwrap();

    app.handle_key(key(KeyCode::Char('w'))).unwrap();

    assert_eq!(app.detail_cursor(), 1);
}

#[test]
fn x_on_the_project_pane_closes_the_selected_open_project() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );

    let Command::Finish(Selection::CloseProject { project }) =
        app.handle_key(key(KeyCode::Char('x'))).unwrap()
    else {
        panic!("x should finish with a close-project selection");
    };
    assert_eq!(project.id, "docs");
}

#[test]
fn x_on_a_host_workspace_closes_the_exact_workspace() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(host_workspace_context()),
        InitialView::Projects,
    );

    let Command::Finish(Selection::CloseWorkspace { workspace }) =
        app.handle_key(key(KeyCode::Char('x'))).unwrap()
    else {
        panic!("x should finish with a close-workspace selection");
    };
    assert_eq!(workspace, "default");
}

#[test]
fn slash_search_treats_x_as_query_text_and_escape_keeps_the_query() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );

    assert_eq!(
        app.handle_key(key(KeyCode::Char('/'))).unwrap(),
        Command::None
    );
    assert_eq!(app.input_mode(), InputMode::Search);
    assert_eq!(
        app.handle_key(key(KeyCode::Char('x'))).unwrap(),
        Command::None
    );
    assert_eq!(app.project_query(), "x");

    app.handle_key(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.input_mode(), InputMode::Normal);
    assert_eq!(app.project_query(), "x");
}

#[test]
fn q_cancels_from_normal_mode() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );

    assert_eq!(
        app.handle_key(key(KeyCode::Char('q'))).unwrap(),
        Command::Cancel
    );
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
fn wide_renderer_shows_projects_and_window_metadata_side_by_side() {
    let app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let lines = rendered_lines(&terminal);
    let rendered = lines.join("\n");

    assert!(rendered.contains("WISP"));
    assert!(rendered.contains("◆ Documentation"));
    assert!(rendered.contains("◆ docs-shell"));
    assert!(rendered.contains("docs/"));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Projects") && line.contains("Windows"))
    );
    assert!(rendered.contains("/ search"));
    assert!(rendered.contains("x close"));
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() == "◆" && cell.fg == Color::Green)
    );
}

#[test]
fn renderer_uses_the_accent_border_for_only_the_focused_pane() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let title_row = rendered_lines(&terminal)
        .iter()
        .position(|line| line.contains("Projects") && line.contains("Windows"))
        .unwrap() as u16;
    assert_eq!(terminal.backend().buffer()[(0, title_row)].fg, Color::Cyan);
    assert_eq!(
        terminal.backend().buffer()[(40, title_row)].fg,
        Color::DarkGray
    );

    app.handle_key(key(KeyCode::Right)).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    assert_eq!(
        terminal.backend().buffer()[(0, title_row)].fg,
        Color::DarkGray
    );
    assert_eq!(terminal.backend().buffer()[(40, title_row)].fg, Color::Cyan);
}

#[test]
fn narrow_renderer_stacks_projects_above_windows() {
    let app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    let backend = TestBackend::new(60, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let lines = rendered_lines(&terminal);
    let projects_row = lines
        .iter()
        .position(|line| line.contains("Projects") && !line.contains("WISP"))
        .unwrap();
    let windows_row = lines
        .iter()
        .position(|line| line.contains("Windows"))
        .unwrap();

    assert!(windows_row > projects_row);
    assert!(lines.iter().any(|line| line.contains("Documentation")));
    assert!(lines.iter().any(|line| line.contains("docs-shell")));
}

#[test]
fn renderer_explains_when_an_open_project_has_no_windows() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 3,
        "projects": {
            "docs": { "labels": ["current", "open"], "items": [] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Projects,
    );
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    assert!(rendered_lines(&terminal).join("\n").contains("No windows"));
}

#[test]
fn renderer_explains_when_the_selected_project_is_not_open() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 3,
        "projects": {
            "api": { "labels": ["new"], "items": [] },
            "web": { "labels": ["open"], "items": [] },
            "docs": { "labels": ["current", "open"], "items": [] }
        },
        "workspaces": {}
    }))
    .unwrap();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Down)).unwrap();
    app.handle_key(key(KeyCode::Down)).unwrap();
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    assert!(
        rendered_lines(&terminal)
            .join("\n")
            .contains("Project is not open")
    );
}

#[test]
fn renderer_distinguishes_filtered_windows_from_an_empty_project() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );
    app.handle_key(key(KeyCode::Char('/'))).unwrap();
    app.handle_key(key(KeyCode::Char('z'))).unwrap();
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    assert!(
        rendered_lines(&terminal)
            .join("\n")
            .contains("No matching windows")
    );
}

#[test]
fn renderer_distinguishes_filtered_files_from_an_empty_directory() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/docs/README.md"),
        EntryKind::File,
    )]);
    app.handle_key(key(KeyCode::Char('/'))).unwrap();
    app.handle_key(key(KeyCode::Char('z'))).unwrap();
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    assert!(
        rendered_lines(&terminal)
            .join("\n")
            .contains("No matching files")
    );
}

#[test]
fn starts_on_the_current_project_with_only_its_windows_visible() {
    let app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );

    assert_eq!(app.focus(), Focus::Projects);
    assert_eq!(app.right_mode(), RightMode::Windows);
    assert_eq!(app.selected_project_id(), Some("docs"));
    assert_eq!(
        app.visible_project_labels(),
        vec!["Documentation", "Web Client", "API Service"]
    );
    assert_eq!(app.visible_detail_labels(), vec!["editor", "docs-shell"]);
}
