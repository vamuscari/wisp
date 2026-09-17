use std::path::{Path, PathBuf};

use wisp_core::{
    config::Openers,
    model::{DirectoryEntry, EntryKind, Project},
    navigation::{NavigationOutcome, Navigator, Screen},
    opencode::{OpenCodeSession, SessionActivity, SessionWaiting},
    protocol::{
        FileHostTarget, FileOpenTarget, HostContext, NvimPaneStateEnvelope, NvimView, NvimViewport,
        OpenCodeStatusEnvelope, PROTOCOL_VERSION, ProjectsEnvelope, Selection, SelectionEnvelope,
        SelectionStatus,
    },
};

fn project() -> Project {
    Project {
        id: "api".into(),
        path: PathBuf::from("/repos/api"),
        group: "Repos".into(),
        name: "api".into(),
        display_name: "API".into(),
    }
}

fn nvim_view(window_id: &str, path: &str) -> NvimView {
    NvimView {
        window_id: window_id.into(),
        path: path.into(),
        active: false,
        width: 120,
        height: 40,
        bottomline: 30,
        view: NvimViewport {
            lnum: 12,
            col: 0,
            coladd: 0,
            curswant: 0,
            topline: 4,
            topfill: 0,
            leftcol: 0,
            skipcol: 0,
        },
    }
}

#[test]
fn navigation_moves_between_projects_and_lazy_directories() {
    let mut navigator = Navigator::new(vec![project()], false);
    assert_eq!(navigator.screen(), &Screen::Projects);

    assert_eq!(
        navigator.browse_project("api").unwrap(),
        NavigationOutcome::LoadDirectory {
            project: project(),
            path: PathBuf::from("/repos/api")
        }
    );

    let src = DirectoryEntry::new(PathBuf::from("/repos/api/src"), EntryKind::Directory);
    assert_eq!(
        navigator
            .select_entry(
                &src,
                &Openers::default(),
                FileOpenTarget::Window,
                false,
                None,
            )
            .unwrap(),
        NavigationOutcome::LoadDirectory {
            project: project(),
            path: PathBuf::from("/repos/api/src")
        }
    );
    assert_eq!(
        navigator.back().unwrap(),
        NavigationOutcome::LoadDirectory {
            project: project(),
            path: PathBuf::from("/repos/api")
        }
    );
    assert_eq!(navigator.back().unwrap(), NavigationOutcome::Continue);
    assert_eq!(navigator.screen(), &Screen::Projects);
    assert_eq!(navigator.back().unwrap(), NavigationOutcome::Cancelled);
}

#[test]
fn file_selection_expands_safe_argv_placeholders() {
    let mut navigator = Navigator::new(vec![project()], false);
    navigator.browse_project("api").unwrap();
    let openers = Openers {
        file: Some(vec![
            "editor".into(),
            "--root".into(),
            "{project.path}".into(),
            "{path}".into(),
            "{project.id}".into(),
        ]),
        project: None,
    };
    let entry = DirectoryEntry::new(PathBuf::from("/repos/api/src/main.rs"), EntryKind::File);

    let host_target = FileHostTarget {
        window_id: "17".into(),
        pane_id: "42".into(),
    };
    let outcome = navigator
        .select_entry(
            &entry,
            &openers,
            FileOpenTarget::RightPane,
            true,
            Some(host_target.clone()),
        )
        .unwrap();
    let NavigationOutcome::Selected(Selection::File {
        project,
        path,
        opener,
        open_target,
        reuse_existing,
        host_target: selected_host_target,
    }) = outcome
    else {
        panic!("expected a file selection")
    };
    assert_eq!(project.id, "api");
    assert_eq!(path, Path::new("/repos/api/src/main.rs"));
    assert_eq!(open_target, FileOpenTarget::RightPane);
    assert!(reuse_existing);
    assert_eq!(selected_host_target, Some(host_target));
    assert_eq!(
        opener.unwrap(),
        vec![
            "editor",
            "--root",
            "/repos/api",
            "/repos/api/src/main.rs",
            "api"
        ]
    );
}

#[test]
fn selecting_project_returns_project_selection_and_opener() {
    let navigator = Navigator::new(vec![project()], false);
    let openers = Openers {
        file: None,
        project: Some(vec!["shell".into(), "{project.path}".into()]),
    };

    let outcome = navigator.select_project("api", &openers).unwrap();
    assert_eq!(
        outcome,
        NavigationOutcome::Selected(Selection::Project {
            project: project(),
            opener: Some(vec!["shell".into(), "/repos/api".into()]),
        })
    );
}

#[test]
fn host_pane_and_project_close_are_direct_navigation_selections() {
    let navigator = Navigator::new(vec![project()], false);

    assert_eq!(
        navigator.select_host_pane("api", "17", "42").unwrap(),
        NavigationOutcome::Selected(Selection::HostPane {
            project: project(),
            window_id: "17".into(),
            pane_id: "42".into(),
        })
    );
    assert_eq!(
        navigator.close_project("api").unwrap(),
        NavigationOutcome::Selected(Selection::CloseProject { project: project() })
    );
}

#[test]
fn opencode_session_selection_carries_resolved_attach_argv_and_optional_host_item() {
    let selection = Selection::OpenCodeSession {
        project: project(),
        session_id: "ses_123".into(),
        opener: vec![
            "opencode".into(),
            "attach".into(),
            "http://127.0.0.1:4096".into(),
            "--dir".into(),
            "/repos/api".into(),
            "--session".into(),
            "ses_123".into(),
        ],
        host_item_id: Some("17".into()),
    };

    let encoded = serde_json::to_value(SelectionEnvelope::selected(selection.clone())).unwrap();
    assert_eq!(encoded["protocol_version"], 8);
    assert_eq!(encoded["selection"]["kind"], "open_code_session");
    assert_eq!(encoded["selection"]["session_id"], "ses_123");
    assert_eq!(encoded["selection"]["host_item_id"], "17");

    let decoded: SelectionEnvelope = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.selection, Some(selection));
}

#[test]
fn navigator_builds_an_opencode_attach_selection_without_a_shell() {
    let navigator = Navigator::new(vec![project()], false);
    let session = OpenCodeSession {
        id: "ses_123".into(),
        title: "Fix API".into(),
        directory: PathBuf::from("/repos/api/services"),
        server_url: "http://127.0.0.1:4096".into(),
        agent: Some("build".into()),
        parent_id: None,
        updated_at: 42,
        activity: SessionActivity::Idle,
        waiting: SessionWaiting::default(),
    };

    let outcome = navigator
        .select_opencode_session("api", &session, &["opencode".into()], Some("17"))
        .unwrap();

    assert_eq!(
        outcome,
        NavigationOutcome::Selected(Selection::OpenCodeSession {
            project: project(),
            session_id: "ses_123".into(),
            opener: vec![
                "opencode",
                "attach",
                "http://127.0.0.1:4096",
                "--dir",
                "/repos/api/services",
                "--session",
                "ses_123",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            host_item_id: Some("17".into()),
        })
    );
}

#[test]
fn host_context_maps_opencode_sessions_to_exact_host_items() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "session_items": {
                    "ses_123": "17"
                }
            }
        },
        "workspaces": {}
    }))
    .unwrap();

    assert_eq!(context.session_item("api", "ses_123"), Some("17"));
    assert_eq!(context.session_item("api", "missing"), None);
    assert_eq!(context.session_item("missing", "ses_123"), None);
}

#[test]
fn selection_envelope_round_trips_as_versioned_json() {
    let envelope = SelectionEnvelope::selected(Selection::File {
        project: project(),
        path: PathBuf::from("/repos/api/README.md"),
        opener: Some(vec!["nvim".into(), "/repos/api/README.md".into()]),
        open_target: FileOpenTarget::Window,
        reuse_existing: false,
        host_target: None,
    });

    let json = serde_json::to_value(&envelope).unwrap();
    assert_eq!(json["protocol_version"], PROTOCOL_VERSION);
    assert_eq!(json["status"], "selected");
    assert_eq!(json["selection"]["kind"], "file");
    assert_eq!(json["selection"]["project"]["id"], "api");
    assert_eq!(json["selection"]["path"], "/repos/api/README.md");

    let decoded: SelectionEnvelope = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, envelope);
    assert_eq!(
        SelectionEnvelope::cancelled().status,
        SelectionStatus::Cancelled
    );
    assert_eq!(
        SelectionEnvelope::error("broken").error.as_deref(),
        Some("broken")
    );
}

#[test]
fn host_context_contains_nested_windows_and_panes_keyed_by_project_id() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "windows": [
                    {
                        "id": "17",
                        "label": "nvim",
                        "detail": "src/main.rs",
                        "active": true,
                        "panes": [
                            { "id": "42", "label": "editor", "active": true },
                            { "id": "43", "label": "terminal" }
                        ]
                    },
                    {
                        "id": "18",
                        "label": "server",
                        "panes": [{ "id": "44", "label": "server" }]
                    }
                ]
            },
            "web": { "labels": ["new"], "windows": [] }
        },
        "workspaces": {}
    }))
    .unwrap();

    assert_eq!(context.labels("api"), &["current", "open"]);
    assert_eq!(context.windows("api")[0].id, "17");
    assert_eq!(context.windows("api")[0].label, "nvim");
    assert_eq!(
        context.windows("api")[0].detail.as_deref(),
        Some("src/main.rs")
    );
    assert_eq!(context.windows("api")[0].panes[0].id, "42");
    assert!(context.windows("api")[0].panes[0].active);
    assert!(context.windows("api")[0].active);
    assert!(!context.windows("api")[1].active);
    assert!(context.labels("missing").is_empty());
    assert!(context.windows("missing").is_empty());
    let encoded = serde_json::to_value(context).unwrap();
    assert_eq!(encoded["protocol_version"], PROTOCOL_VERSION);
    assert!(
        encoded["projects"]["api"]["windows"][0]["panes"][0]
            .get("nvim_views")
            .is_none()
    );
}

#[test]
fn host_context_contains_open_host_workspaces() {
    let context = serde_json::from_value::<HostContext>(serde_json::json!({
        "protocol_version": 8,
        "projects": {},
        "workspaces": {
            "default": {
                "current": true,
                "windows": [
                    {
                        "id": "17",
                        "label": "shell",
                        "active": true,
                        "panes": [{ "id": "42", "label": "shell", "active": true }]
                    }
                ]
            }
        }
    }));

    assert!(
        context.is_ok(),
        "open host workspaces should be valid context"
    );
    let encoded = serde_json::to_value(context.unwrap()).unwrap();
    assert_eq!(encoded["workspaces"]["default"]["current"], true);
    assert_eq!(encoded["workspaces"]["default"]["windows"][0]["id"], "17");
}

#[test]
fn host_context_requires_the_v8_workspace_collection() {
    let context = serde_json::from_value::<HostContext>(serde_json::json!({
        "protocol_version": 8,
        "projects": {}
    }));

    assert!(context.is_err(), "v8 host context must include workspaces");
}

#[test]
fn host_context_defaults_omitted_windows_to_empty() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": { "labels": ["new"] }
        },
        "workspaces": {}
    }))
    .unwrap();

    assert!(context.windows("api").is_empty());
}

#[test]
fn host_context_rejects_unsupported_versions_and_invalid_hierarchy() {
    let unsupported = serde_json::from_value::<HostContext>(serde_json::json!({
        "protocol_version": 1,
        "projects": [],
        "future_field": true
    }))
    .unwrap_err();
    assert!(
        unsupported
            .to_string()
            .contains("unsupported host context version 1"),
        "unexpected error: {unsupported}"
    );

    let empty = serde_json::from_value::<HostContext>(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["open"],
                "windows": [{
                    "id": "",
                    "label": "nvim",
                    "panes": [{ "id": "42", "label": "editor" }]
                }]
            }
        },
        "workspaces": {}
    }));
    assert!(empty.is_err());

    let duplicate = serde_json::from_value::<HostContext>(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["open"],
                "windows": [
                    { "id": "17", "label": "nvim", "panes": [{ "id": "42", "label": "editor" }] },
                    { "id": "17", "label": "shell", "panes": [{ "id": "43", "label": "shell" }] }
                ]
            }
        },
        "workspaces": {}
    }));
    assert!(duplicate.is_err());

    let empty_pane = serde_json::from_value::<HostContext>(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["open"],
                "windows": [{
                    "id": "17",
                    "label": "nvim",
                    "panes": [{ "id": "", "label": "editor" }]
                }]
            }
        },
        "workspaces": {}
    }));
    assert!(empty_pane.is_err());

    let duplicate_project = serde_json::from_str::<HostContext>(
        r#"{"protocol_version":8,"projects":{"api":{"labels":["new"]},"api":{"labels":["open"]}},"workspaces":{}}"#,
    );
    assert!(duplicate_project.is_err());

    let duplicate_workspace = serde_json::from_str::<HostContext>(
        r#"{"protocol_version":8,"projects":{},"workspaces":{"default":{"current":true},"default":{"current":false}}}"#,
    );
    assert!(duplicate_workspace.is_err());
}

#[test]
fn public_protocol_fixtures_decode_with_the_current_models() {
    let selection: SelectionEnvelope = serde_json::from_str(include_str!(
        "../../../tests/fixtures/selection-file-v8.json"
    ))
    .unwrap();
    assert_eq!(selection.protocol_version, PROTOCOL_VERSION);
    assert_eq!(selection.status, SelectionStatus::Selected);

    let context: HostContext =
        serde_json::from_str(include_str!("../../../tests/fixtures/host-context-v8.json")).unwrap();
    assert_eq!(context.labels("api"), &["current", "open"]);
    assert_eq!(context.windows("api")[0].id, "17");
    assert_eq!(context.windows("api")[0].panes[0].nvim_views.len(), 1);
    assert!(context.workspaces().contains_key("default"));

    let projects: ProjectsEnvelope =
        serde_json::from_str(include_str!("../../../tests/fixtures/projects-v8.json")).unwrap();
    assert_eq!(projects.projects[0].id, "api");

    let session: SelectionEnvelope = serde_json::from_str(include_str!(
        "../../../tests/fixtures/selection-open-code-session-v8.json"
    ))
    .unwrap();
    assert!(matches!(
        session.selection,
        Some(Selection::OpenCodeSession { ref session_id, .. }) if session_id == "ses_123"
    ));

    let status: OpenCodeStatusEnvelope = serde_json::from_str(include_str!(
        "../../../tests/fixtures/opencode-status-v8.json"
    ))
    .unwrap();
    assert_eq!(status.sessions.waiting, 1);
}

#[test]
fn opencode_status_envelope_is_strict_and_checks_version_first() {
    let unsupported = serde_json::from_value::<OpenCodeStatusEnvelope>(serde_json::json!({
        "protocol_version": 1,
        "sessions": "future schema",
        "future_field": true
    }))
    .unwrap_err();
    assert!(
        unsupported
            .to_string()
            .contains("unsupported OpenCode status protocol version 1"),
        "unexpected error: {unsupported}"
    );

    let unknown = serde_json::from_value::<OpenCodeStatusEnvelope>(serde_json::json!({
        "protocol_version": 8,
        "sessions": {
            "waiting": 0,
            "running": 0,
            "retrying": 0,
            "idle": 0,
            "error": 0,
            "future_field": true
        }
    }));
    assert!(unknown.is_err());
}

#[test]
fn projects_envelope_checks_version_before_the_project_schema() {
    let unsupported = serde_json::from_value::<ProjectsEnvelope>(serde_json::json!({
        "protocol_version": 1,
        "projects": "future schema",
        "future_field": true
    }))
    .unwrap_err();
    assert!(
        unsupported
            .to_string()
            .contains("unsupported projects protocol version 1"),
        "unexpected error: {unsupported}"
    );

    let current = serde_json::from_value::<ProjectsEnvelope>(serde_json::json!({
        "protocol_version": 8,
        "projects": [project()]
    }))
    .unwrap();
    assert_eq!(current.projects, vec![project()]);
}

#[test]
fn selection_protocol_rejects_unknown_project_fields() {
    let selection = serde_json::from_value::<SelectionEnvelope>(serde_json::json!({
        "protocol_version": 8,
        "status": "selected",
        "selection": {
            "kind": "project",
            "project": {
                "id": "api",
                "path": "/repos/api",
                "group": "Repos",
                "name": "api",
                "display_name": "API",
                "future_field": true
            }
        }
    }));

    assert!(selection.is_err());
}

#[test]
fn selection_protocol_rejects_unknown_selection_fields() {
    let selection = serde_json::from_value::<SelectionEnvelope>(serde_json::json!({
        "protocol_version": 8,
        "status": "selected",
        "selection": {
            "kind": "project",
            "project": project(),
            "future_field": true
        }
    }));

    assert!(selection.is_err());
}

#[test]
fn selection_protocol_rejects_duplicate_envelope_fields() {
    let duplicate = serde_json::from_str::<SelectionEnvelope>(
        r#"{"protocol_version":8,"protocol_version":8,"status":"cancelled"}"#,
    )
    .unwrap_err();

    assert!(
        duplicate.to_string().contains("duplicate field"),
        "unexpected error: {duplicate}"
    );
}

#[test]
fn selection_protocol_rejects_inconsistent_status_fields() {
    for json in [
        r#"{"protocol_version":8,"status":"selected"}"#,
        r#"{"protocol_version":8,"status":"cancelled","selection":{"kind":"project","project":{"id":"api","path":"/repos/api","group":"Repos","name":"api","display_name":"API"}}}"#,
        r#"{"protocol_version":8,"status":"error"}"#,
    ] {
        assert!(
            serde_json::from_str::<SelectionEnvelope>(json).is_err(),
            "inconsistent envelope should fail: {json}"
        );
    }
}

#[test]
fn close_project_is_a_versioned_host_action_selection() {
    let decoded = serde_json::from_value::<Selection>(serde_json::json!({
        "kind": "close_project",
        "project": project()
    }));

    assert!(
        decoded.is_ok(),
        "close_project should be part of protocol v8"
    );
    let encoded = serde_json::to_value(SelectionEnvelope::selected(decoded.unwrap())).unwrap();
    assert_eq!(encoded["selection"]["kind"], "close_project");
}

#[test]
fn host_pane_is_a_versioned_selection_with_opaque_ids() {
    let selection = Selection::HostPane {
        project: project(),
        window_id: "17".into(),
        pane_id: "42".into(),
    };

    let encoded = serde_json::to_value(SelectionEnvelope::selected(selection.clone())).unwrap();
    assert_eq!(encoded["protocol_version"], 8);
    assert_eq!(encoded["selection"]["kind"], "host_pane");
    assert_eq!(encoded["selection"]["window_id"], "17");
    assert_eq!(encoded["selection"]["pane_id"], "42");

    let decoded: SelectionEnvelope = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.selection, Some(selection));
}

#[test]
fn host_workspace_actions_are_versioned_selections() {
    for selection in [
        serde_json::json!({
            "kind": "workspace",
            "workspace": "default"
        }),
        serde_json::json!({
            "kind": "workspace_pane",
            "workspace": "default",
            "window_id": "17",
            "pane_id": "42"
        }),
        serde_json::json!({
            "kind": "close_workspace",
            "workspace": "default"
        }),
    ] {
        let decoded = serde_json::from_value::<Selection>(selection.clone());
        assert!(
            decoded.is_ok(),
            "host workspace action should decode: {selection}"
        );
        let envelope = SelectionEnvelope::selected(decoded.unwrap());
        assert_eq!(
            serde_json::to_value(envelope).unwrap()["protocol_version"],
            PROTOCOL_VERSION
        );
    }
}

#[test]
fn protocol_v8_file_selection_carries_all_open_targets_and_optional_host_target() {
    assert_eq!(PROTOCOL_VERSION, 8);

    for (target, encoded_target) in [
        (FileOpenTarget::Window, "window"),
        (FileOpenTarget::RightPane, "right_pane"),
        (FileOpenTarget::BottomPane, "bottom_pane"),
    ] {
        let selection = Selection::File {
            project: project(),
            path: "/repos/api/src/main.rs".into(),
            opener: None,
            open_target: target,
            reuse_existing: true,
            host_target: Some(FileHostTarget {
                window_id: "17".into(),
                pane_id: "42".into(),
            }),
        };
        let encoded = serde_json::to_value(SelectionEnvelope::selected(selection.clone())).unwrap();
        assert_eq!(encoded["selection"]["open_target"], encoded_target);
        assert_eq!(encoded["selection"]["reuse_existing"], true);
        assert_eq!(encoded["selection"]["host_target"]["window_id"], "17");
        assert_eq!(encoded["selection"]["host_target"]["pane_id"], "42");
        assert_eq!(
            serde_json::from_value::<SelectionEnvelope>(encoded)
                .unwrap()
                .selection,
            Some(selection)
        );
    }

    let without_host = Selection::File {
        project: project(),
        path: "/repos/api/src/main.rs".into(),
        opener: None,
        open_target: FileOpenTarget::Window,
        reuse_existing: false,
        host_target: None,
    };
    let encoded = serde_json::to_value(SelectionEnvelope::selected(without_host)).unwrap();
    assert!(encoded["selection"].get("host_target").is_none());
}

#[test]
fn protocol_v8_file_selection_requires_open_policy_fields() {
    let mut selection = serde_json::json!({
        "protocol_version": 8,
        "status": "selected",
        "selection": {
            "kind": "file",
            "project": project(),
            "path": "/repos/api/src/main.rs",
            "open_target": "window",
            "reuse_existing": false
        }
    });

    for required in ["open_target", "reuse_existing"] {
        let removed = selection["selection"]
            .as_object_mut()
            .unwrap()
            .remove(required);
        assert!(
            serde_json::from_value::<SelectionEnvelope>(selection.clone()).is_err(),
            "file selection without {required} should fail"
        );
        selection["selection"].as_object_mut().unwrap().insert(
            required.into(),
            removed.expect("required test field should exist"),
        );
    }

    selection["selection"]["host_target"] = serde_json::json!({
        "window_id": "17",
        "pane_id": "42",
        "future_field": true
    });
    assert!(serde_json::from_value::<SelectionEnvelope>(selection).is_err());

    for field in ["window_id", "pane_id"] {
        let mut selection = serde_json::json!({
            "protocol_version": 8,
            "status": "selected",
            "selection": {
                "kind": "file",
                "project": project(),
                "path": "/repos/api/src/main.rs",
                "open_target": "window",
                "reuse_existing": true,
                "host_target": { "window_id": "17", "pane_id": "42" }
            }
        });
        selection["selection"]["host_target"][field] = "".into();
        assert!(
            serde_json::from_value::<SelectionEnvelope>(selection).is_err(),
            "empty file host target {field} should fail"
        );
    }
}

#[test]
fn protocol_v8_rejects_v7_envelopes_before_decoding_their_payloads() {
    let selection = serde_json::from_value::<SelectionEnvelope>(serde_json::json!({
        "protocol_version": 7,
        "status": "future status",
        "future_field": true
    }))
    .unwrap_err();
    assert!(
        selection
            .to_string()
            .contains("unsupported selection protocol version 7")
    );

    let pane_state = serde_json::from_value::<NvimPaneStateEnvelope>(serde_json::json!({
        "protocol_version": 7,
        "views": "future schema",
        "future_field": true
    }))
    .unwrap_err();
    assert!(
        pane_state
            .to_string()
            .contains("unsupported Neovim pane state protocol version 7")
    );
}

#[test]
fn nvim_pane_state_round_trips_and_requires_view_activity() {
    let mut json = serde_json::json!({
        "protocol_version": 8,
        "views": [{
            "window_id": "1001",
            "path": "/repos/api/src/main.rs",
            "active": false,
            "width": 120,
            "height": 40,
            "bottomline": 30,
            "view": {
                "lnum": 12,
                "col": 0,
                "coladd": 0,
                "curswant": 0,
                "topline": 4,
                "topfill": 0,
                "leftcol": 0,
                "skipcol": 0
            }
        }]
    });

    let mut missing_active = json.clone();
    missing_active["views"][0]
        .as_object_mut()
        .unwrap()
        .remove("active");
    assert!(serde_json::from_value::<NvimPaneStateEnvelope>(missing_active).is_err());

    let state: NvimPaneStateEnvelope = serde_json::from_value(json.take()).unwrap();
    assert_eq!(state.protocol_version, PROTOCOL_VERSION);
    assert_eq!(
        state.views,
        vec![nvim_view("1001", "/repos/api/src/main.rs")]
    );
    assert_eq!(
        serde_json::from_value::<NvimPaneStateEnvelope>(serde_json::to_value(&state).unwrap())
            .unwrap(),
        state
    );
}

#[test]
fn nvim_pane_state_accepts_cross_platform_absolute_paths() {
    let state = serde_json::json!({
        "protocol_version": 8,
        "views": [
            nvim_view("1001", "/repos/api/src/main.rs"),
            nvim_view("1002", r"C:\repos\api\src\lib.rs"),
            nvim_view("1003", r"\\server\share\api\README.md")
        ]
    });

    assert!(serde_json::from_value::<NvimPaneStateEnvelope>(state).is_ok());
}

#[test]
fn nvim_pane_state_rejects_unknown_duplicate_and_invalid_views() {
    let valid_view = serde_json::to_value(nvim_view("1001", "/repos/api/src/main.rs")).unwrap();
    let mut unknown = serde_json::json!({
        "protocol_version": 8,
        "views": [valid_view.clone()]
    });
    unknown["views"][0]["future_field"] = true.into();
    assert!(serde_json::from_value::<NvimPaneStateEnvelope>(unknown).is_err());

    let duplicate = r#"{"protocol_version":8,"views":[],"views":[]}"#;
    assert!(serde_json::from_str::<NvimPaneStateEnvelope>(duplicate).is_err());

    for (field, invalid) in [
        ("window_id", serde_json::json!("")),
        ("path", serde_json::json!("")),
        ("path", serde_json::json!("relative/file.rs")),
        ("width", serde_json::json!(0)),
        ("height", serde_json::json!(0)),
        ("bottomline", serde_json::json!(0)),
    ] {
        let mut view = valid_view.clone();
        view[field] = invalid;
        let envelope = serde_json::json!({"protocol_version": 8, "views": [view]});
        assert!(
            serde_json::from_value::<NvimPaneStateEnvelope>(envelope).is_err(),
            "invalid {field} should fail"
        );
    }

    for (field, invalid) in [
        ("lnum", serde_json::json!(0)),
        ("topline", serde_json::json!(0)),
        ("col", serde_json::json!(-1)),
    ] {
        let mut view = valid_view.clone();
        view["view"][field] = invalid;
        let envelope = serde_json::json!({"protocol_version": 8, "views": [view]});
        assert!(
            serde_json::from_value::<NvimPaneStateEnvelope>(envelope).is_err(),
            "invalid view {field} should fail"
        );
    }

    let mut reversed_range = valid_view.clone();
    reversed_range["view"]["topline"] = 20.into();
    reversed_range["bottomline"] = 19.into();
    assert!(
        serde_json::from_value::<NvimPaneStateEnvelope>(serde_json::json!({
            "protocol_version": 8,
            "views": [reversed_range]
        }))
        .is_err()
    );

    assert!(
        serde_json::from_value::<NvimPaneStateEnvelope>(serde_json::json!({
            "protocol_version": 8,
            "views": [valid_view.clone(), valid_view]
        }))
        .is_err(),
        "duplicate Neovim window IDs should fail"
    );
}

#[test]
fn host_pane_nvim_views_are_optional_strict_and_semantically_validated() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 8,
        "projects": {
            "api": {
                "labels": ["open"],
                "windows": [{
                    "id": "17",
                    "label": "nvim",
                    "panes": [{
                        "id": "42",
                        "label": "editor",
                        "nvim_views": [nvim_view("1001", "/repos/api/src/main.rs")]
                    }]
                }]
            }
        },
        "workspaces": {}
    }))
    .unwrap();
    assert_eq!(context.windows("api")[0].panes[0].nvim_views.len(), 1);

    let encoded = serde_json::to_value(
        serde_json::from_value::<HostContext>(serde_json::json!({
            "protocol_version": 8,
            "projects": {"api": {"labels": ["new"]}},
            "workspaces": {}
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(
        encoded["projects"]["api"]["windows"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    for views in [
        vec![nvim_view("1001", "relative/file.rs")],
        vec![
            nvim_view("1001", "/repos/api/a.rs"),
            nvim_view("1001", "/repos/api/b.rs"),
        ],
    ] {
        let invalid = serde_json::json!({
            "protocol_version": 8,
            "projects": {
                "api": {
                    "labels": ["open"],
                    "windows": [{
                        "id": "17",
                        "label": "nvim",
                        "panes": [{"id": "42", "label": "editor", "nvim_views": views}]
                    }]
                }
            },
            "workspaces": {}
        });
        assert!(serde_json::from_value::<HostContext>(invalid).is_err());
    }
}
