use std::path::{Path, PathBuf};

use wisp_core::{
    config::Openers,
    model::{DirectoryEntry, EntryKind, Project},
    navigation::{NavigationOutcome, Navigator, Screen},
    protocol::{
        HostContext, PROTOCOL_VERSION, ProjectsEnvelope, Selection, SelectionEnvelope,
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
        navigator.select_entry(&src, &Openers::default()).unwrap(),
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

    let outcome = navigator.select_entry(&entry, &openers).unwrap();
    let NavigationOutcome::Selected(Selection::File {
        project,
        path,
        opener,
    }) = outcome
    else {
        panic!("expected a file selection")
    };
    assert_eq!(project.id, "api");
    assert_eq!(path, Path::new("/repos/api/src/main.rs"));
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
fn host_item_and_project_close_are_direct_navigation_selections() {
    let navigator = Navigator::new(vec![project()], false);

    assert_eq!(
        navigator.select_host_item("api", "17").unwrap(),
        NavigationOutcome::Selected(Selection::HostItem {
            project: project(),
            id: "17".into(),
        })
    );
    assert_eq!(
        navigator.close_project("api").unwrap(),
        NavigationOutcome::Selected(Selection::CloseProject { project: project() })
    );
}

#[test]
fn selection_envelope_round_trips_as_versioned_json() {
    let envelope = SelectionEnvelope::selected(Selection::File {
        project: project(),
        path: PathBuf::from("/repos/api/README.md"),
        opener: Some(vec!["nvim".into(), "/repos/api/README.md".into()]),
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
fn host_context_contains_labels_and_items_keyed_by_project_id() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 2,
        "projects": {
            "api": {
                "labels": ["current", "open"],
                "items": [
                    {
                        "id": "17",
                        "label": "nvim",
                        "detail": "src/main.rs",
                        "active": true
                    },
                    {
                        "id": "18",
                        "label": "server"
                    }
                ]
            },
            "web": { "labels": ["new"], "items": [] }
        }
    }))
    .unwrap();

    assert_eq!(context.labels("api"), &["current", "open"]);
    assert_eq!(context.items("api")[0].id, "17");
    assert_eq!(context.items("api")[0].label, "nvim");
    assert_eq!(
        context.items("api")[0].detail.as_deref(),
        Some("src/main.rs")
    );
    assert!(context.items("api")[0].active);
    assert!(!context.items("api")[1].active);
    assert!(context.labels("missing").is_empty());
    assert!(context.items("missing").is_empty());
    assert_eq!(
        serde_json::to_value(context).unwrap()["protocol_version"],
        PROTOCOL_VERSION
    );
}

#[test]
fn host_context_defaults_omitted_items_to_empty() {
    let context: HostContext = serde_json::from_value(serde_json::json!({
        "protocol_version": 2,
        "projects": {
            "api": { "labels": ["new"] }
        }
    }))
    .unwrap();

    assert!(context.items("api").is_empty());
}

#[test]
fn host_context_rejects_unsupported_versions_and_invalid_items() {
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
        "protocol_version": 2,
        "projects": {
            "api": {
                "labels": ["open"],
                "items": [{ "id": "", "label": "nvim" }]
            }
        }
    }));
    assert!(empty.is_err());

    let duplicate = serde_json::from_value::<HostContext>(serde_json::json!({
        "protocol_version": 2,
        "projects": {
            "api": {
                "labels": ["open"],
                "items": [
                    { "id": "17", "label": "nvim" },
                    { "id": "17", "label": "shell" }
                ]
            }
        }
    }));
    assert!(duplicate.is_err());

    let duplicate_project = serde_json::from_str::<HostContext>(
        r#"{"protocol_version":2,"projects":{"api":{"labels":["new"]},"api":{"labels":["open"]}}}"#,
    );
    assert!(duplicate_project.is_err());
}

#[test]
fn public_protocol_fixtures_decode_with_the_current_models() {
    let selection: SelectionEnvelope = serde_json::from_str(include_str!(
        "../../../tests/fixtures/selection-file-v2.json"
    ))
    .unwrap();
    assert_eq!(selection.protocol_version, PROTOCOL_VERSION);
    assert_eq!(selection.status, SelectionStatus::Selected);

    let context: HostContext =
        serde_json::from_str(include_str!("../../../tests/fixtures/host-context-v2.json")).unwrap();
    assert_eq!(context.labels("api"), &["current", "open"]);
    assert_eq!(context.items("api")[0].id, "17");

    let projects: ProjectsEnvelope =
        serde_json::from_str(include_str!("../../../tests/fixtures/projects-v2.json")).unwrap();
    assert_eq!(projects.projects[0].id, "api");
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
        "protocol_version": 2,
        "projects": [project()]
    }))
    .unwrap();
    assert_eq!(current.projects, vec![project()]);
}

#[test]
fn selection_protocol_rejects_unknown_project_fields() {
    let selection = serde_json::from_value::<SelectionEnvelope>(serde_json::json!({
        "protocol_version": 2,
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
        "protocol_version": 2,
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
        r#"{"protocol_version":2,"protocol_version":2,"status":"cancelled"}"#,
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
        r#"{"protocol_version":2,"status":"selected"}"#,
        r#"{"protocol_version":2,"status":"cancelled","selection":{"kind":"project","project":{"id":"api","path":"/repos/api","group":"Repos","name":"api","display_name":"API"}}}"#,
        r#"{"protocol_version":2,"status":"error"}"#,
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
        "close_project should be part of protocol v2"
    );
    let encoded = serde_json::to_value(SelectionEnvelope::selected(decoded.unwrap())).unwrap();
    assert_eq!(encoded["selection"]["kind"], "close_project");
}

#[test]
fn host_item_is_a_versioned_selection_with_an_opaque_id() {
    let selection = Selection::HostItem {
        project: project(),
        id: "17".into(),
    };

    let encoded = serde_json::to_value(SelectionEnvelope::selected(selection.clone())).unwrap();
    assert_eq!(encoded["protocol_version"], 2);
    assert_eq!(encoded["selection"]["kind"], "host_item");
    assert_eq!(encoded["selection"]["id"], "17");

    let decoded: SelectionEnvelope = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.selection, Some(selection));
}
