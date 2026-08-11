use std::path::PathBuf;

use wisp_core::opencode::{
    OpenCodeSession, OpenCodeStatusCounts, SessionActivity, SessionDisplayState, SessionWaiting,
};

fn session(activity: SessionActivity, waiting: SessionWaiting) -> OpenCodeSession {
    OpenCodeSession {
        id: "ses_root".into(),
        title: "Implement OpenCode integration".into(),
        directory: PathBuf::from("/repos/wisp"),
        server_url: "http://127.0.0.1:4096".into(),
        agent: Some("build".into()),
        parent_id: None,
        updated_at: 42,
        activity,
        waiting,
    }
}

#[test]
fn waiting_reasons_take_precedence_over_activity() {
    let both = session(
        SessionActivity::Running,
        SessionWaiting {
            permissions: 2,
            questions: 1,
        },
    );
    assert_eq!(
        both.display_state(),
        SessionDisplayState::Waiting {
            permissions: 2,
            questions: 1,
        }
    );

    let permission = session(
        SessionActivity::Idle,
        SessionWaiting {
            permissions: 1,
            questions: 0,
        },
    );
    assert_eq!(
        permission.display_state(),
        SessionDisplayState::Waiting {
            permissions: 1,
            questions: 0,
        }
    );
}

#[test]
fn session_type_combines_agent_and_hierarchy() {
    let root = session(SessionActivity::Idle, SessionWaiting::default());
    assert_eq!(root.type_label(), "build · root");

    let mut child = root;
    child.agent = None;
    child.parent_id = Some("ses_parent".into());
    assert_eq!(child.type_label(), "unassigned · child");
}

#[test]
fn activity_is_preserved_when_nothing_is_waiting() {
    let retrying = session(
        SessionActivity::Retrying {
            attempt: 3,
            message: "rate limited".into(),
            next_at: 99,
        },
        SessionWaiting::default(),
    );
    assert_eq!(
        retrying.display_state(),
        SessionDisplayState::Retrying {
            attempt: 3,
            message: "rate limited".into(),
            next_at: 99,
        }
    );
}

#[test]
fn status_counts_use_display_state_precedence() {
    let sessions = vec![
        session(
            SessionActivity::Running,
            SessionWaiting {
                permissions: 0,
                questions: 1,
            },
        ),
        session(SessionActivity::Running, SessionWaiting::default()),
        session(
            SessionActivity::Retrying {
                attempt: 2,
                message: "rate limited".into(),
                next_at: 99,
            },
            SessionWaiting::default(),
        ),
        session(SessionActivity::Idle, SessionWaiting::default()),
        session(
            SessionActivity::Error {
                message: "provider failed".into(),
            },
            SessionWaiting::default(),
        ),
    ];

    assert_eq!(
        OpenCodeStatusCounts::from_sessions(&sessions),
        OpenCodeStatusCounts {
            waiting: 1,
            running: 1,
            retrying: 1,
            idle: 1,
            error: 1,
        }
    );
}
