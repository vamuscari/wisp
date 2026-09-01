use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};
use wisp_core::{
    config::{Openers, VcsIcons},
    model::{DirectoryEntry, EntryKind, Project},
    opencode::{OpenCodeSession, OpenCodeSnapshot, SessionActivity, SessionWaiting},
    protocol::{FileHostTarget, FilePreviewState, HostContext, Selection},
};
use wisp_tui::{
    ActiveProjectContext, App, AuxiliaryPane, Command, Focus, GitSummary, InitialView, InputMode,
    RightMode, SinglePaneBehavior, render,
};

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
        "protocol_version": 7,
        "projects": {
            "api": {
                "labels": ["new"],
                "windows": [{
                    "id": "11",
                    "label": "api-shell",
                    "panes": [{ "id": "61", "label": "api-shell" }]
                }]
            },
            "web": {
                "labels": ["open"],
                "windows": [{
                    "id": "12",
                    "label": "web-server",
                    "panes": [{ "id": "62", "label": "web-server" }]
                }]
            },
            "docs": {
                "labels": ["current", "open"],
                "windows": [
                    {
                        "id": "17",
                        "label": "editor",
                        "panes": [{ "id": "71", "label": "editor" }]
                    },
                    {
                        "id": "18",
                        "label": "docs-shell",
                        "detail": "docs/",
                        "active": true,
                        "panes": [
                            { "id": "72", "label": "shell", "active": true },
                            { "id": "73", "label": "logs" }
                        ]
                    }
                ]
            }
        },
        "workspaces": {}
    }))
    .unwrap()
}

fn host_workspace_context() -> HostContext {
    serde_json::from_value(serde_json::json!({
        "protocol_version": 7,
        "projects": {
            "api": { "labels": ["open"] },
            "web": { "labels": ["new"] },
            "docs": { "labels": ["new"] }
        },
        "workspaces": {
            "default": {
                "current": true,
                "windows": [
                    {
                        "id": "29",
                        "label": "shell",
                        "active": true,
                        "panes": [{ "id": "43", "label": "shell", "active": true }]
                    }
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
        "protocol_version": 7,
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
        "protocol_version": 7,
        "projects": {
            "api": { "labels": ["open"], "windows": [] },
            "web": { "labels": ["new"], "windows": [] },
            "docs": { "labels": ["new"], "windows": [] }
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
fn arrows_and_tab_change_hierarchical_focus() {
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
fn o_on_the_project_pane_returns_the_selected_project() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );

    let Command::Finish(Selection::Project { project, opener }) =
        app.handle_key(key(KeyCode::Char('o'))).unwrap()
    else {
        panic!("project open should finish with a project selection");
    };
    assert_eq!(project.id, "docs");
    assert_eq!(opener, None);
}

#[test]
fn o_on_a_host_workspace_returns_the_exact_workspace() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(host_workspace_context()),
        InitialView::Projects,
    );

    let Command::Finish(Selection::Workspace { workspace }) =
        app.handle_key(key(KeyCode::Char('o'))).unwrap()
    else {
        panic!("workspace open should finish with a workspace selection");
    };
    assert_eq!(workspace, "default");
}

#[test]
fn enter_descends_from_windows_and_returns_the_selected_host_pane() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );

    assert_eq!(app.handle_key(key(KeyCode::Enter)).unwrap(), Command::None);
    let Command::Finish(Selection::HostPane {
        project,
        window_id,
        pane_id,
    }) = app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("pane enter should finish with a host-pane selection");
    };
    assert_eq!(project.id, "docs");
    assert_eq!(window_id, "18");
    assert_eq!(pane_id, "72");
}

#[test]
fn enter_on_a_host_workspace_descends_to_the_exact_pane() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(host_workspace_context()),
        InitialView::Windows,
    );

    assert_eq!(app.handle_key(key(KeyCode::Enter)).unwrap(), Command::None);
    let Command::Finish(Selection::WorkspacePane {
        workspace,
        window_id,
        pane_id,
    }) = app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("workspace pane enter should finish with an exact host target");
    };
    assert_eq!(workspace, "default");
    assert_eq!(window_id, "29");
    assert_eq!(pane_id, "43");
}

#[test]
fn configured_single_pane_activation_skips_the_pane_column() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(host_workspace_context()),
        InitialView::Windows,
    );
    app.configure_single_pane_behavior(SinglePaneBehavior::Activate);

    let Command::Finish(Selection::WorkspacePane {
        workspace,
        window_id,
        pane_id,
    }) = app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("single-pane window should activate directly");
    };
    assert_eq!(workspace, "default");
    assert_eq!(window_id, "29");
    assert_eq!(pane_id, "43");
}

#[test]
fn pane_search_cannot_make_a_multi_pane_window_activate_directly() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );
    app.configure_single_pane_behavior(SinglePaneBehavior::Activate);
    app.handle_key(key(KeyCode::Enter)).unwrap();
    app.handle_key(key(KeyCode::Char('/'))).unwrap();
    for character in "logs".chars() {
        app.handle_key(key(KeyCode::Char(character))).unwrap();
    }
    app.handle_key(key(KeyCode::Esc)).unwrap();
    app.handle_key(key(KeyCode::Left)).unwrap();

    assert_eq!(app.handle_key(key(KeyCode::Enter)).unwrap(), Command::None);
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
    assert_eq!(
        app.handle_key(key(KeyCode::Down)).unwrap(),
        Command::LoadDirectory(PathBuf::from("/repos/docs/src"))
    );
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/docs/src/lib.rs"),
        EntryKind::File,
    )]);
    assert_eq!(app.handle_key(key(KeyCode::Enter)).unwrap(), Command::None);
    assert_eq!(
        app.current_directory(),
        Some(PathBuf::from("/repos/docs/src").as_path())
    );
    assert_eq!(app.visible_detail_labels(), vec!["lib.rs"]);
}

#[test]
fn file_preview_and_enter_use_the_exact_ranked_nvim_target() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 7,
        "projects": {
            "docs": {
                "labels": ["current", "open"],
                "windows": [
                    {
                        "id": "inactive-window", "label": "inactive",
                        "panes": [{
                            "id": "active-pane", "label": "editor", "active": true,
                            "nvim_views": [{
                                "window_id": "active-view", "path": "/repos/docs/README.md",
                                "active": true, "width": 80, "height": 20, "bottomline": 20,
                                "view": { "lnum": 1, "col": 0, "coladd": 0, "curswant": 0, "topline": 1, "topfill": 0, "leftcol": 0, "skipcol": 0 }
                            }]
                        }]
                    },
                    {
                        "id": "active-window", "label": "active", "active": true,
                        "panes": [
                            {
                                "id": "inactive-pane", "label": "editor",
                                "nvim_views": [{
                                    "window_id": "active-view", "path": "/repos/docs/README.md",
                                    "active": true, "width": 80, "height": 20, "bottomline": 20,
                                    "view": { "lnum": 2, "col": 0, "coladd": 0, "curswant": 0, "topline": 1, "topfill": 0, "leftcol": 0, "skipcol": 0 }
                                }]
                            },
                            {
                                "id": "ranked-pane", "label": "editor", "active": true,
                                "nvim_views": [
                                    {
                                        "window_id": "inactive-view", "path": "/repos/docs/README.md",
                                        "active": false,
                                        "width": 80, "height": 20, "bottomline": 20,
                                        "view": { "lnum": 3, "col": 0, "coladd": 0, "curswant": 0, "topline": 1, "topfill": 0, "leftcol": 0, "skipcol": 0 }
                                    },
                                    {
                                        "window_id": "ranked-view", "path": "/repos/docs/README.md",
                                        "active": true, "width": 80, "height": 20, "bottomline": 20,
                                        "view": { "lnum": 4, "col": 0, "coladd": 0, "curswant": 0, "topline": 1, "topfill": 0, "leftcol": 0, "skipcol": 0 }
                                    },
                                    {
                                        "window_id": "later-active-view", "path": "/repos/docs/README.md",
                                        "active": true, "width": 80, "height": 20, "bottomline": 20,
                                        "view": { "lnum": 5, "col": 0, "coladd": 0, "curswant": 0, "topline": 1, "topfill": 0, "leftcol": 0, "skipcol": 0 }
                                    }
                                ]
                            }
                        ]
                    }
                ]
            }
        },
        "workspaces": {}
    }))
    .unwrap();
    let expected_view = context.windows("docs")[1].panes[1].nvim_views[1].clone();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Projects,
    );
    app.configure_file_preview(true, true);
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/docs/README.md"),
        EntryKind::File,
    )]);

    assert_eq!(
        app.file_preview_state(),
        FilePreviewState::File {
            project: projects()[2].clone(),
            path: PathBuf::from("/repos/docs/README.md"),
            nvim_view: Some(Box::new(expected_view)),
        }
    );
    let Command::Finish(Selection::File { host_target, .. }) =
        app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("file enter should finish");
    };
    assert_eq!(
        host_target,
        Some(FileHostTarget {
            window_id: "active-window".into(),
            pane_id: "ranked-pane".into(),
        })
    );
}

#[test]
fn file_matching_normalizes_windows_drive_and_unc_paths() {
    for (project_path, entry_path, view_path) in [
        (
            r"C:\Repos\Docs",
            r"C:\REPOS\DOCS\README.md",
            r"c:/repos/docs/src/../README.md",
        ),
        (
            r"\\Server\Share\Docs",
            r"\\SERVER\SHARE\DOCS\README.md",
            r"//server/share/docs/./README.md",
        ),
    ] {
        let project = Project {
            id: "docs".into(),
            path: PathBuf::from(project_path),
            group: "Repos".into(),
            name: "docs".into(),
            display_name: "Documentation".into(),
        };
        let context: HostContext = serde_json::from_value(serde_json::json!({
            "protocol_version": 7,
            "projects": {
                "docs": {
                    "labels": ["current", "open"],
                    "windows": [{
                        "id": "17", "label": "editor", "active": true,
                        "panes": [{
                            "id": "71", "label": "editor", "active": true,
                            "nvim_views": [{
                                "window_id": "9", "path": view_path, "active": true,
                                "width": 80, "height": 20, "bottomline": 20,
                                "view": { "lnum": 1, "col": 0, "coladd": 0, "curswant": 0, "topline": 1, "topfill": 0, "leftcol": 0, "skipcol": 0 }
                            }]
                        }]
                    }]
                }
            },
            "workspaces": {}
        }))
        .unwrap();
        let mut app = App::new(
            vec![project],
            Openers::default(),
            false,
            Some(context),
            InitialView::Projects,
        );
        app.configure_file_preview(true, true);
        app.handle_key(key(KeyCode::Char('f'))).unwrap();
        app.load_directory(vec![DirectoryEntry::new(
            PathBuf::from(entry_path),
            EntryKind::File,
        )]);

        let FilePreviewState::File { nvim_view, .. } = app.file_preview_state() else {
            panic!("selected file should be previewed");
        };
        assert_eq!(
            nvim_view.as_ref().map(|view| view.window_id.as_str()),
            Some("9")
        );
    }
}

#[test]
fn file_matching_excludes_other_projects_and_host_workspaces() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 7,
        "projects": {
            "docs": { "labels": ["current", "open"] },
            "api": {
                "labels": ["open"],
                "windows": [{
                    "id": "other-window", "label": "editor",
                    "panes": [{
                        "id": "other-pane", "label": "editor",
                        "nvim_views": [{
                            "window_id": "1", "path": "/repos/docs/README.md",
                            "active": true, "width": 80, "height": 20, "bottomline": 20,
                            "view": { "lnum": 1, "col": 0, "coladd": 0, "curswant": 0, "topline": 1, "topfill": 0, "leftcol": 0, "skipcol": 0 }
                        }]
                    }]
                }]
            }
        },
        "workspaces": {
            "host-only": {
                "current": false,
                "windows": [{
                    "id": "workspace-window", "label": "editor",
                    "panes": [{
                        "id": "workspace-pane", "label": "editor",
                        "nvim_views": [{
                            "window_id": "2", "path": "/repos/docs/README.md",
                            "active": false,
                            "width": 80, "height": 20, "bottomline": 20,
                            "view": { "lnum": 1, "col": 0, "coladd": 0, "curswant": 0, "topline": 1, "topfill": 0, "leftcol": 0, "skipcol": 0 }
                        }]
                    }]
                }]
            }
        }
    }))
    .unwrap();
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context),
        InitialView::Projects,
    );
    app.configure_file_preview(true, true);
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/docs/README.md"),
        EntryKind::File,
    )]);

    let FilePreviewState::File { nvim_view, .. } = app.file_preview_state() else {
        panic!("selected file should be previewed");
    };
    assert_eq!(nvim_view, None);
    let Command::Finish(Selection::File { host_target, .. }) =
        app.handle_key(key(KeyCode::Enter)).unwrap()
    else {
        panic!("file enter should finish");
    };
    assert_eq!(host_target, None);
}

#[test]
fn file_preview_visibility_tracks_selection_toggles_and_mode_transitions() {
    let mut app = App::new_with_opencode(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
        vec!["opencode".into()],
    );
    app.configure_file_preview(true, true);

    assert_eq!(app.file_preview_state(), FilePreviewState::Hidden);
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    assert_eq!(app.file_preview_state(), FilePreviewState::Empty);
    app.load_directory(vec![
        DirectoryEntry::new(PathBuf::from("/repos/docs/src"), EntryKind::Directory),
        DirectoryEntry::new(PathBuf::from("/repos/docs/README.md"), EntryKind::File),
    ]);
    assert!(matches!(
        app.file_preview_state(),
        FilePreviewState::File { .. }
    ));

    app.handle_key(key(KeyCode::Char('w'))).unwrap();
    assert_eq!(app.file_preview_state(), FilePreviewState::Hidden);
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/docs/README.md"),
        EntryKind::File,
    )]);
    assert!(matches!(
        app.file_preview_state(),
        FilePreviewState::File { .. }
    ));

    app.handle_key(key(KeyCode::Char('p'))).unwrap();
    assert_eq!(app.file_preview_state(), FilePreviewState::Hidden);
    app.handle_key(key(KeyCode::Char('s'))).unwrap();
    assert_eq!(app.file_preview_state(), FilePreviewState::Hidden);
    app.clear_status();
    app.handle_key(key(KeyCode::Char('p'))).unwrap();
    assert_eq!(app.status(), Some("File preview is unavailable"));
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/docs/README.md"),
        EntryKind::File,
    )]);
    assert_eq!(app.file_preview_state(), FilePreviewState::Hidden);
}

#[test]
fn file_preview_is_empty_for_a_highlighted_directory() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.configure_file_preview(true, true);
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/docs/src"),
        EntryKind::Directory,
    )]);

    assert_eq!(app.file_preview_state(), FilePreviewState::Empty);
}

#[test]
fn file_rows_render_a_live_target_marker_without_changing_labels() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 7,
        "projects": {
            "docs": {
                "labels": ["current", "open"],
                "windows": [{
                    "id": "17", "label": "editor",
                    "panes": [{
                        "id": "71", "label": "editor",
                        "nvim_views": [{
                            "window_id": "9", "path": "/repos/docs/README.md",
                            "active": false,
                            "width": 80, "height": 20, "bottomline": 20,
                            "view": { "lnum": 1, "col": 0, "coladd": 0, "curswant": 0, "topline": 1, "topfill": 0, "leftcol": 0, "skipcol": 0 }
                        }]
                    }]
                }]
            }
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
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![
        DirectoryEntry::new(PathBuf::from("/repos/docs/README.md"), EntryKind::File),
        DirectoryEntry::new(PathBuf::from("/repos/docs/src"), EntryKind::Directory),
    ]);
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");

    assert_eq!(app.visible_detail_labels(), vec!["README.md", "src/"]);
    assert!(rendered.contains("◆ README.md"));
    assert!(!rendered.contains("◆ src/"));
}

#[test]
fn file_search_ranks_the_closest_match_first() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.load_directory(vec![
        DirectoryEntry::new(PathBuf::from("/repos/docs/a---b"), EntryKind::File),
        DirectoryEntry::new(PathBuf::from("/repos/docs/ab"), EntryKind::File),
    ]);
    app.handle_key(key(KeyCode::Char('/'))).unwrap();
    app.handle_key(key(KeyCode::Char('a'))).unwrap();
    app.handle_key(key(KeyCode::Char('b'))).unwrap();

    assert_eq!(app.visible_detail_labels(), vec!["ab", "a---b"]);
}

fn deeply_browsed_files_app() -> App {
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
    assert_eq!(
        app.handle_key(key(KeyCode::Enter)).unwrap(),
        Command::LoadDirectory(PathBuf::from("/repos/docs/src"))
    );
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/docs/src/components"),
        EntryKind::Directory,
    )]);
    assert_eq!(
        app.handle_key(key(KeyCode::Enter)).unwrap(),
        Command::LoadDirectory(PathBuf::from("/repos/docs/src/components"))
    );
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/docs/src/components/button.rs"),
        EntryKind::File,
    )]);
    app
}

#[test]
fn file_columns_adapt_from_four_levels_to_a_recycled_narrow_view() {
    let app = deeply_browsed_files_app();

    let backend = TestBackend::new(180, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    let wide_title = &rendered_lines(&terminal)[0];
    assert!(wide_title.contains(" Projects "));
    assert!(wide_title.contains(" docs "));
    assert!(wide_title.contains(" src "));
    assert!(wide_title.contains(" components "));
    assert!(
        rendered_lines(&terminal)
            .join("\n")
            .contains("NORMAL  Projects > docs > src > components")
    );

    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    let medium_title = &rendered_lines(&terminal)[0];
    assert!(medium_title.contains(" Projects "));
    assert!(!medium_title.contains(" docs "));
    assert!(medium_title.contains(" src "));
    assert!(medium_title.contains(" components "));

    let backend = TestBackend::new(60, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    let narrow_lines = rendered_lines(&terminal);
    let narrow = narrow_lines[..21].join("\n");
    assert!(narrow.contains(" Projects "));
    assert!(!narrow.contains(" src "));
    assert!(narrow.contains(" components "));
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
        Command::None
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
fn backspace_cancels_a_pending_deep_descent_without_skipping_its_parent() {
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
    app.load_directory(vec![DirectoryEntry::new(
        PathBuf::from("/repos/docs/src/components"),
        EntryKind::Directory,
    )]);
    assert_eq!(
        app.handle_key(key(KeyCode::Enter)).unwrap(),
        Command::LoadDirectory(PathBuf::from("/repos/docs/src/components"))
    );

    assert_eq!(
        app.handle_key(key(KeyCode::Backspace)).unwrap(),
        Command::None
    );
    assert_eq!(app.focus(), Focus::Detail);
    assert_eq!(
        app.current_directory(),
        Some(PathBuf::from("/repos/docs/src").as_path())
    );
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
fn window_preview_target_follows_the_highlighted_window_when_enabled() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );

    assert_eq!(app.window_preview_target(), None);
    app.configure_window_preview(true, true);
    assert_eq!(app.window_preview_target(), Some("72".into()));

    app.handle_key(key(KeyCode::Up)).unwrap();
    assert_eq!(app.window_preview_target(), Some("71".into()));
}

#[test]
fn preview_is_hidden_by_default_and_toggled_with_p() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );
    app.configure_window_preview(true, false);

    assert_eq!(app.auxiliary_pane(), AuxiliaryPane::Hidden);
    assert_eq!(app.window_preview_target(), None);

    app.handle_key(key(KeyCode::Char('p'))).unwrap();
    assert_eq!(app.auxiliary_pane(), AuxiliaryPane::Preview);
    assert_eq!(app.window_preview_target(), Some("72".into()));

    app.handle_key(key(KeyCode::Char('p'))).unwrap();
    assert_eq!(app.auxiliary_pane(), AuxiliaryPane::Hidden);
    assert_eq!(app.window_preview_target(), None);
}

#[test]
fn commands_restore_the_previous_auxiliary_pane() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );
    app.configure_window_preview(true, true);

    app.handle_key(key(KeyCode::Char('?'))).unwrap();
    assert_eq!(app.auxiliary_pane(), AuxiliaryPane::Commands);
    assert_eq!(app.window_preview_target(), None);

    app.handle_key(key(KeyCode::Esc)).unwrap();
    assert_eq!(app.auxiliary_pane(), AuxiliaryPane::Preview);
    assert_eq!(app.window_preview_target(), Some("72".into()));

    app.handle_key(key(KeyCode::Char('p'))).unwrap();
    app.handle_key(key(KeyCode::Char('?'))).unwrap();
    app.handle_key(key(KeyCode::Char('?'))).unwrap();
    assert_eq!(app.auxiliary_pane(), AuxiliaryPane::Hidden);
}

#[test]
fn preview_and_commands_keys_are_search_text_in_search_mode() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.configure_window_preview(true, false);

    app.handle_key(key(KeyCode::Char('/'))).unwrap();
    app.handle_key(key(KeyCode::Char('p'))).unwrap();
    app.handle_key(key(KeyCode::Char('?'))).unwrap();

    assert_eq!(app.project_query(), "p?");
    assert_eq!(app.auxiliary_pane(), AuxiliaryPane::Hidden);
}

#[test]
fn mouse_hover_changes_only_the_window_preview_target() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );
    app.configure_window_preview(true, true);
    assert_eq!(app.detail_cursor(), 1);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 42,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 24),
    );

    assert_eq!(
        app.detail_cursor(),
        1,
        "hover must not move keyboard selection"
    );
    assert_eq!(app.window_preview_target(), Some("71".into()));

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 90,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 200, 24),
    );
    assert_eq!(
        app.window_preview_target(),
        Some("72".into()),
        "hovering the Pane column must keep the keyboard-selected window target"
    );

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 42,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 24),
    );
    assert_eq!(
        app.detail_cursor(),
        1,
        "clicks must not activate or select windows"
    );

    app.handle_key(key(KeyCode::Down)).unwrap();
    assert_eq!(app.window_preview_target(), Some("72".into()));
}

#[test]
fn moving_up_to_another_window_clears_the_previous_pane_search() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );
    app.handle_key(key(KeyCode::Enter)).unwrap();
    app.handle_key(key(KeyCode::Char('/'))).unwrap();
    for character in "logs".chars() {
        app.handle_key(key(KeyCode::Char(character))).unwrap();
    }
    app.handle_key(key(KeyCode::Esc)).unwrap();
    app.handle_key(key(KeyCode::Left)).unwrap();
    app.handle_key(key(KeyCode::Up)).unwrap();

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    assert!(
        !rendered_lines(&terminal)
            .join("\n")
            .contains("No matching panes")
    );
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

    assert!(!rendered.contains("WISP"));
    assert!(rendered.contains("◆ Documentation"));
    assert!(rendered.contains("◆ docs-shell"));
    assert!(rendered.contains("docs/"));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Projects") && line.contains("Windows"))
    );
    assert!(rendered.contains("NORMAL  Projects > Windows"));
    assert!(!rendered.contains("/ search"));
    assert!(!rendered.contains("x close"));
    assert_eq!(terminal.backend().buffer()[(0, 21)].fg, Color::White);
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
fn wide_windows_view_renders_a_separate_panes_column() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );
    app.handle_key(key(KeyCode::Enter)).unwrap();
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");

    assert!(rendered.contains("Panes"));
    assert!(rendered.contains("shell"));
    assert!(rendered.contains("logs"));
    assert!(rendered.contains("Projects > Windows > Panes"));
}

#[test]
fn bottom_utility_bar_becomes_the_focused_search_input() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('/'))).unwrap();
    app.handle_key(key(KeyCode::Char('a'))).unwrap();
    app.handle_key(key(KeyCode::Char('p'))).unwrap();
    app.handle_key(key(KeyCode::Char('i'))).unwrap();
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");

    assert!(rendered.contains("SEARCH  Projects  / api"));
    assert!(!rendered.contains("NORMAL  Projects > Windows"));
}

#[test]
fn commands_render_in_the_preview_slot() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );
    app.configure_window_preview(true, false);
    app.handle_key(key(KeyCode::Char('?'))).unwrap();
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");

    assert!(rendered.contains("Commands"));
    assert!(rendered.contains("p File/Window Preview"));
    assert!(rendered.contains("Ctrl-R Refresh"));
    assert!(!rendered.contains("Preview unavailable"));
}

#[test]
fn commands_show_default_and_forced_file_open_targets() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.handle_key(key(KeyCode::Char('f'))).unwrap();
    app.handle_key(key(KeyCode::Char('?'))).unwrap();
    let backend = TestBackend::new(140, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");

    assert!(rendered.contains("Enter Default"));
    assert!(rendered.contains("Ctrl-T Window"));
    assert!(rendered.contains("Ctrl-V Right"));
    assert!(rendered.contains("Ctrl-X Bottom"));
    assert!(rendered.contains("p File/Window Preview"));
    assert!(rendered.contains("w/f/s View"));
    assert!(rendered.contains("Ctrl-R Refresh"));
}

#[test]
fn commands_replace_the_detail_pane_when_height_is_constrained() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );
    app.configure_window_preview(true, false);
    app.handle_key(key(KeyCode::Char('?'))).unwrap();
    let backend = TestBackend::new(60, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let lines = rendered_lines(&terminal);
    let rendered = lines.join("\n");

    assert!(rendered.contains("Projects"));
    assert!(rendered.contains("Commands"));
    assert!(!lines[..7].iter().any(|line| line.contains("Windows")));
}

#[test]
fn commands_keep_the_detail_pane_at_the_wide_layout_boundary() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Windows,
    );
    app.configure_window_preview(true, false);
    app.handle_key(key(KeyCode::Char('?'))).unwrap();
    let backend = TestBackend::new(72, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let content = rendered_lines(&terminal)[..21].join("\n");

    assert!(content.contains("Projects"));
    assert!(content.contains("Windows"));
    assert!(content.contains("Commands"));
}

#[test]
fn utility_bar_shows_status_when_not_searching() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.set_status("directory unavailable");
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();

    assert!(
        rendered_lines(&terminal)
            .join("\n")
            .contains("ERROR  directory unavailable")
    );
}

#[test]
fn active_project_file_and_git_status_render_inline_without_adding_a_window() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.set_active_project_context(ActiveProjectContext {
        project_id: "api".into(),
        file: Some("src/main.rs".into()),
        git: Some(GitSummary {
            branch: "main".into(),
            dirty: true,
            modified: 1,
            ..GitSummary::default()
        }),
    });
    let backend = TestBackend::new(160, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let lines = rendered_lines(&terminal);
    let rendered = lines.join("\n");

    assert_eq!(app.selected_project_id(), Some("api"));
    assert_eq!(app.visible_detail_labels(), vec!["api-shell"]);
    assert!(rendered.contains("◆ API Service  src/main.rs"));
    assert!(rendered.contains("main ✗ !1"));
    assert!(!rendered.contains("src/main.rs  api-shell"));
}

#[test]
fn vcs_icons_render_counts_and_combined_divergence_by_default() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.set_active_project_context(ActiveProjectContext {
        project_id: "api".into(),
        file: None,
        git: Some(GitSummary {
            branch: "main".into(),
            dirty: true,
            untracked: 2,
            modified: 1,
            staged: 3,
            conflicted: 1,
            ahead: 2,
            behind: 1,
            stashed: 4,
        }),
    });
    let backend = TestBackend::new(200, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");

    assert!(rendered.contains("◆ API Service"));
    assert!(
        rendered.contains("main ✗ ?2 !1 +3 ×1 ⇕2/1 *4"),
        "every applicable VCS indicator should render in order:\n{rendered}"
    );
}

#[test]
fn vcs_icons_can_be_overridden_or_disabled() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    let icons = VcsIcons {
        dirty: None,
        untracked: Some("U".into()),
        modified: None,
        staged: Some("S".into()),
        stashed: None,
        ..VcsIcons::default()
    };
    app.set_vcs_icons(icons);
    app.set_active_project_context(ActiveProjectContext {
        project_id: "api".into(),
        file: None,
        git: Some(GitSummary {
            branch: "main".into(),
            dirty: true,
            untracked: 2,
            modified: 1,
            staged: 3,
            stashed: 4,
            ..GitSummary::default()
        }),
    });
    let backend = TestBackend::new(160, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");

    assert!(
        rendered.contains("main U2 S3"),
        "custom icons should render:\n{rendered}"
    );
    assert!(!rendered.contains("✗"));
    assert!(!rendered.contains("!1"));
    assert!(!rendered.contains("*4"));
}

#[test]
fn editor_active_project_is_not_treated_as_host_open() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        None,
        InitialView::Projects,
    );
    app.set_active_project_context(ActiveProjectContext {
        project_id: "web".into(),
        file: Some("src/app.rs".into()),
        git: None,
    });

    assert_eq!(app.selected_project_id(), Some("web"));
    assert_eq!(
        app.handle_key(key(KeyCode::Char('x'))).unwrap(),
        Command::None
    );

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");

    assert!(rendered.contains("◆ Web Client  src/app.rs"));
    assert!(rendered.contains("Project is not open"));
}

#[test]
fn git_update_for_an_open_inactive_project_renders_on_its_row() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.set_active_project_context(ActiveProjectContext {
        project_id: "docs".into(),
        file: None,
        git: None,
    });

    app.set_active_project_git(
        "web",
        GitSummary {
            branch: "feature/web".into(),
            dirty: true,
            ..GitSummary::default()
        },
    );

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");

    assert!(rendered.contains("● Web Client"));
    assert!(rendered.contains("feature/web ✗"));
}

#[test]
fn replacing_projects_clears_stale_git_summaries() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.set_active_project_git(
        "web",
        GitSummary {
            branch: "stale-branch".into(),
            dirty: true,
            ..GitSummary::default()
        },
    );

    app.replace_projects(projects());

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");
    assert!(!rendered.contains("stale-branch"));
}

#[test]
fn active_project_row_keeps_the_filename_and_clean_git_state_when_space_is_limited() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.set_active_project_context(ActiveProjectContext {
        project_id: "api".into(),
        file: Some("a/very/long/project/relative/path/src/main.rs".into()),
        git: Some(GitSummary {
            branch: "main".into(),
            dirty: false,
            ..GitSummary::default()
        }),
    });
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let lines = rendered_lines(&terminal);
    let rendered = lines.join("\n");

    assert!(
        rendered.contains("◆ API Service  …/src/main.rs  main ✓"),
        "the file should truncate from the left without hiding Git state:\n{rendered}"
    );
}

#[test]
fn long_git_branch_truncates_before_the_right_aligned_state() {
    let mut app = App::new(
        projects(),
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.set_active_project_context(ActiveProjectContext {
        project_id: "api".into(),
        file: None,
        git: Some(GitSummary {
            branch: "feature/an-excessively-long-branch-name".into(),
            dirty: true,
            ..GitSummary::default()
        }),
    });
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let lines = rendered_lines(&terminal);
    let rendered = lines.join("\n");

    assert!(
        rendered.contains("◆ API Service  feature/an-… ✗"),
        "the branch should truncate before the state:\n{rendered}"
    );
}

#[test]
fn long_project_name_truncates_before_the_right_aligned_git_metadata() {
    let mut projects = projects();
    projects[0].display_name = "An extremely long project name".into();
    let mut app = App::new(
        projects,
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.set_active_project_context(ActiveProjectContext {
        project_id: "api".into(),
        file: None,
        git: Some(GitSummary {
            branch: "main".into(),
            dirty: true,
            ..GitSummary::default()
        }),
    });
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered = rendered_lines(&terminal).join("\n");

    assert!(
        rendered.contains("main ✗"),
        "Git metadata should remain visible after the project name truncates:\n{rendered}"
    );
    assert!(!rendered.contains("An extremely long project name"));
}

#[test]
fn minimum_wide_layout_keeps_the_vcs_state_visible() {
    let mut projects = projects();
    projects[0].display_name = "An extremely long project name".into();
    let mut app = App::new(
        projects,
        Openers::default(),
        false,
        Some(context()),
        InitialView::Projects,
    );
    app.set_active_project_context(ActiveProjectContext {
        project_id: "api".into(),
        file: None,
        git: Some(GitSummary {
            branch: "feature/an-excessively-long-branch-name".into(),
            dirty: true,
            ..GitSummary::default()
        }),
    });
    let backend = TestBackend::new(72, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let project_row = rendered_lines(&terminal)
        .into_iter()
        .find(|line| line.contains("An extremely"))
        .expect("active project row should render");

    assert!(
        project_row.contains("✗"),
        "the primary VCS state should survive the minimum pane width:\n{project_row}"
    );
}

#[test]
fn wide_renderer_uses_an_adaptive_capped_projects_width() {
    for (terminal_width, detail_start) in [(100, 33), (160, 53), (200, 56)] {
        let app = App::new(
            projects(),
            Openers::default(),
            false,
            Some(context()),
            InitialView::Projects,
        );
        let backend = TestBackend::new(terminal_width, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| render(frame, &app)).unwrap();
        let title_row = rendered_lines(&terminal)
            .iter()
            .position(|line| line.contains("Projects") && line.contains("Windows"))
            .unwrap() as u16;

        assert_eq!(
            terminal.backend().buffer()[(detail_start - 1, title_row)].symbol(),
            "┐"
        );
        assert_eq!(
            terminal.backend().buffer()[(detail_start, title_row)].symbol(),
            "┌"
        );
    }
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
        terminal.backend().buffer()[(33, title_row)].fg,
        Color::DarkGray
    );

    app.handle_key(key(KeyCode::Right)).unwrap();
    terminal.draw(|frame| render(frame, &app)).unwrap();
    assert_eq!(
        terminal.backend().buffer()[(0, title_row)].fg,
        Color::DarkGray
    );
    assert_eq!(terminal.backend().buffer()[(33, title_row)].fg, Color::Cyan);
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
        "protocol_version": 7,
        "projects": {
            "docs": { "labels": ["current", "open"], "windows": [] }
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
        "protocol_version": 7,
        "projects": {
            "api": { "labels": ["new"], "windows": [] },
            "web": { "labels": ["open"], "windows": [] },
            "docs": { "labels": ["current", "open"], "windows": [] }
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
