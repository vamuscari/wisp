use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tempfile::TempDir;
use wisp_cli::opencode::{
    OpenCodeClient, OpenCodeError, RegistryRegistration, live_status, register_instance,
    unregister_instance,
};
use wisp_core::{
    config::OpenCodeConfig,
    opencode::{SessionActivity, SessionDisplayState, SessionWaiting},
};

struct FakeServer {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FakeServer {
    fn new(routes: BTreeMap<&'static str, &'static str>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let request_log = Arc::clone(&requests);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let routes = routes
            .into_iter()
            .map(|(path, body)| (path.to_string(), body.to_string()))
            .collect::<BTreeMap<_, _>>();
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("fake server accept failed: {error}"),
                };
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).unwrap();
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                let request = String::from_utf8(request).unwrap();
                let target = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap()
                    .to_string();
                request_log.lock().unwrap().push(target.clone());
                let body = routes
                    .iter()
                    .filter(|(path, _)| target.starts_with(path.as_str()))
                    .max_by_key(|(path, _)| path.len())
                    .map(|(_, body)| body.as_str());
                let (status, body) = body.map_or(("404 Not Found", "{}"), |body| ("200 OK", body));
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        Self {
            address,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(&self.address);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn config(server: &FakeServer) -> OpenCodeConfig {
    OpenCodeConfig {
        server_url: server.url(),
        command: vec!["opencode".into()],
        session_limit: 100,
    }
}

fn routes(sessions: &'static str) -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("/global/health", r#"{"healthy":true,"version":"1.18.15"}"#),
        ("/session/status", r#"{}"#),
        ("/session", sessions),
        ("/permission", r#"[]"#),
        ("/question", r#"[]"#),
    ])
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

#[test]
fn snapshot_combines_activity_permissions_questions_agent_and_hierarchy() {
    let server = FakeServer::new(BTreeMap::from([
        ("/global/health", r#"{"healthy":true,"version":"1.18.15"}"#),
        (
            "/session/status",
            r#"{
                "ses_root":{"type":"busy"},
                "ses_child":{"type":"retry","attempt":2,"message":"rate limited","next":99}
            }"#,
        ),
        (
            "/session",
            r#"[
                {
                    "id":"ses_root","projectID":"project","directory":"/repos/wisp",
                    "title":"Root task","agent":"build","version":"1.18.15",
                    "time":{"created":1,"updated":20}
                },
                {
                    "id":"ses_child","projectID":"project","directory":"/repos/wisp",
                    "parentID":"ses_root","title":"Research","agent":"explore","version":"1.18.15",
                    "time":{"created":2,"updated":21}
                }
            ]"#,
        ),
        (
            "/permission",
            r#"[{"id":"per_1","sessionID":"ses_child"},{"id":"per_2","sessionID":"ses_child"}]"#,
        ),
        (
            "/question",
            r#"[{"id":"que_1","sessionID":"ses_root","questions":[]}]"#,
        ),
    ]));
    let registry = TempDir::new().unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&server), registry.path().to_path_buf());

    let snapshot = client.snapshot(Path::new("/repos/wisp")).unwrap();

    assert_eq!(snapshot.sessions.len(), 2);
    let root = snapshot
        .sessions
        .iter()
        .find(|session| session.id == "ses_root")
        .unwrap();
    assert_eq!(root.agent.as_deref(), Some("build"));
    assert_eq!(root.parent_id, None);
    assert_eq!(root.activity, SessionActivity::Running);
    assert_eq!(
        root.display_state(),
        SessionDisplayState::Waiting {
            permissions: 0,
            questions: 1,
        }
    );
    let child = snapshot
        .sessions
        .iter()
        .find(|session| session.id == "ses_child")
        .unwrap();
    assert_eq!(child.parent_id.as_deref(), Some("ses_root"));
    assert_eq!(child.waiting.permissions, 2);
    assert_eq!(child.server_url, server.url());
    assert!(
        server
            .requests()
            .iter()
            .any(|request| request.starts_with("/session?"))
    );
    assert!(
        server
            .requests()
            .iter()
            .any(|request| request.contains("scope=project"))
    );
}

#[test]
fn server_version_is_checked_before_session_schema() {
    let server = FakeServer::new(BTreeMap::from([(
        "/global/health",
        r#"{"healthy":true,"version":"1.18.14"}"#,
    )]));
    let registry = TempDir::new().unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&server), registry.path().to_path_buf());

    let error = client.snapshot(Path::new("/repos/wisp")).unwrap_err();

    assert!(matches!(
        error,
        OpenCodeError::UnsupportedVersion { ref found } if found == "1.18.14"
    ));
    assert_eq!(server.requests(), vec!["/global/health"]);
}

#[test]
fn server_version_is_checked_before_the_strict_health_schema() {
    let server = FakeServer::new(BTreeMap::from([(
        "/global/health",
        r#"{"healthy":false,"version":"1.18.16","future_health_field":true}"#,
    )]));
    let registry = TempDir::new().unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&server), registry.path().to_path_buf());

    let error = client.snapshot(Path::new("/repos/wisp")).unwrap_err();

    assert!(matches!(
        error,
        OpenCodeError::UnsupportedVersion { ref found } if found == "1.18.16"
    ));
    assert_eq!(server.requests(), vec!["/global/health"]);
}

#[test]
fn registry_adds_unmanaged_servers_and_exact_pane_mappings() {
    let shared = FakeServer::new(routes("[]"));
    let unmanaged = FakeServer::new(routes(
        r#"[{
            "id":"ses_unmanaged","projectID":"project","directory":"/repos/wisp",
            "title":"Unmanaged task","agent":"plan","version":"1.18.15",
            "time":{"created":1,"updated":5}
        }]"#,
    ));
    let registry = TempDir::new().unwrap();
    fs::write(
        registry.path().join("instance.json"),
        serde_json::json!({
            "registry_version": 3,
            "instance_id": "123:/repos/wisp",
            "pid": 123,
            "server_url": unmanaged.url(),
            "directory": "/repos/wisp",
            "project_path": "/repos/wisp",
            "updated_at": now_millis(),
            "pane_id": "42",
            "session_id": "ses_unmanaged",
            "session_activity": "idle",
            "session_waiting": { "permissions": 0, "questions": 0 }
        })
        .to_string(),
    )
    .unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&shared), registry.path().to_path_buf());

    let snapshot = client.snapshot(Path::new("/repos/wisp")).unwrap();

    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.sessions[0].id, "ses_unmanaged");
    assert_eq!(
        snapshot.host_items.get("ses_unmanaged").map(String::as_str),
        Some("pane:42")
    );
}

#[test]
fn registered_session_errors_are_reflected_in_picker_snapshots() {
    let shared = FakeServer::new(routes("[]"));
    let unmanaged = FakeServer::new(routes(
        r#"[{
            "id":"ses_error","projectID":"project","directory":"/repos/wisp",
            "title":"Failed task","agent":"build","version":"1.18.15",
            "time":{"created":1,"updated":5}
        }]"#,
    ));
    let registry = TempDir::new().unwrap();
    register_instance(
        registry.path(),
        &RegistryRegistration {
            server_url: unmanaged.url(),
            directory: PathBuf::from("/repos/wisp"),
            project_path: PathBuf::from("/repos/wisp"),
            pid: 123,
            pane_id: Some("42".into()),
            session_id: Some("ses_error".into()),
            session_activity: Some(SessionActivity::Idle),
            session_waiting: Default::default(),
            session_error: Some("provider failed".into()),
        },
    )
    .unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&shared), registry.path().to_path_buf());

    let snapshot = client.snapshot(Path::new("/repos/wisp")).unwrap();

    assert_eq!(
        snapshot.sessions[0].activity,
        SessionActivity::Error {
            message: "provider failed".into(),
        }
    );
}

#[test]
fn registry_project_matching_uses_windows_path_identity_rules() {
    let shared = FakeServer::new(routes("[]"));
    let unmanaged = FakeServer::new(routes(
        r#"[{
            "id":"ses_windows","projectID":"project","directory":"c:\\repos\\wisp",
            "title":"Windows task","agent":"build","version":"1.18.15",
            "time":{"created":1,"updated":5}
        }]"#,
    ));
    let registry = TempDir::new().unwrap();
    fs::write(
        registry.path().join("instance.json"),
        serde_json::json!({
            "registry_version": 3,
            "instance_id": "123:C:\\Repos\\Wisp",
            "pid": 123,
            "server_url": unmanaged.url(),
            "directory": r"C:\Repos\Wisp",
            "project_path": r"C:\Repos\Wisp",
            "updated_at": now_millis(),
            "session_activity": null,
            "session_waiting": { "permissions": 0, "questions": 0 }
        })
        .to_string(),
    )
    .unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&shared), registry.path().to_path_buf());

    let snapshot = client.snapshot(Path::new(r"c:\repos\wisp")).unwrap();

    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.sessions[0].id, "ses_windows");
}

#[test]
fn transient_unmanaged_server_failure_preserves_its_registration() {
    let shared = FakeServer::new(routes("[]"));
    let unavailable = TcpListener::bind("127.0.0.1:0").unwrap();
    let unavailable_url = format!("http://{}", unavailable.local_addr().unwrap());
    drop(unavailable);
    let registry = TempDir::new().unwrap();
    let path = registry.path().join("instance.json");
    fs::write(
        &path,
        serde_json::json!({
            "registry_version": 3,
            "instance_id": "123:/repos/wisp",
            "pid": 123,
            "server_url": unavailable_url,
            "directory": "/repos/wisp",
            "project_path": "/repos/wisp",
            "updated_at": now_millis(),
            "session_activity": null,
            "session_waiting": { "permissions": 0, "questions": 0 }
        })
        .to_string(),
    )
    .unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&shared), registry.path().to_path_buf());

    let snapshot = client.snapshot(Path::new("/repos/wisp")).unwrap();

    assert!(snapshot.sessions.is_empty());
    assert!(path.exists());
}

#[test]
fn stale_registry_entries_are_discarded_before_session_aggregation() {
    let shared = FakeServer::new(routes("[]"));
    let stale = FakeServer::new(routes(
        r#"[{
            "id":"ses_stale","projectID":"project","directory":"/repos/wisp",
            "title":"Stale task","agent":"build","version":"1.18.15",
            "time":{"created":1,"updated":5}
        }]"#,
    ));
    let registry = TempDir::new().unwrap();
    let path = registry.path().join("instance.json");
    fs::write(
        &path,
        serde_json::json!({
            "registry_version": 3,
            "instance_id": "123:/repos/wisp",
            "pid": 123,
            "server_url": stale.url(),
            "directory": "/repos/wisp",
            "project_path": "/repos/wisp",
            "updated_at": 0,
            "session_activity": null,
            "session_waiting": { "permissions": 0, "questions": 0 }
        })
        .to_string(),
    )
    .unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&shared), registry.path().to_path_buf());

    let snapshot = client.snapshot(Path::new("/repos/wisp")).unwrap();

    assert!(snapshot.sessions.is_empty());
    assert!(!path.exists());
    assert!(stale.requests().is_empty());
}

#[test]
fn incompatible_registry_entries_are_discarded_without_interpretation() {
    let shared = FakeServer::new(routes("[]"));
    let registry = TempDir::new().unwrap();
    let path = registry.path().join("future.json");
    fs::write(
        &path,
        r#"{"registry_version":2,"future_server_shape":true}"#,
    )
    .unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&shared), registry.path().to_path_buf());

    let snapshot = client.snapshot(Path::new("/repos/wisp")).unwrap();

    assert!(snapshot.sessions.is_empty());
    assert!(!path.exists());
}

#[test]
fn current_registry_entries_with_invalid_semantics_are_discarded() {
    let server = FakeServer::new(routes("[]"));
    let registry = TempDir::new().unwrap();
    let path = registry.path().join("invalid.json");
    fs::write(
        &path,
        serde_json::json!({
            "registry_version": 3,
            "instance_id": "0:/repos/wisp",
            "pid": 0,
            "server_url": server.url(),
            "directory": "/repos/wisp",
            "project_path": "/repos/wisp",
            "updated_at": now_millis(),
            "session_id": "ses_invalid",
            "session_activity": "idle",
            "session_waiting": { "permissions": 0, "questions": 0 }
        })
        .to_string(),
    )
    .unwrap();

    let status = live_status(registry.path()).unwrap();

    assert_eq!(status, Default::default());
    assert!(!path.exists());
    assert!(server.requests().is_empty());
}

#[test]
fn registry_entries_from_the_future_are_discarded() {
    let server = FakeServer::new(routes("[]"));
    let registry = TempDir::new().unwrap();
    let path = registry.path().join("future.json");
    fs::write(
        &path,
        serde_json::json!({
            "registry_version": 3,
            "instance_id": "1:/repos/wisp",
            "pid": 1,
            "server_url": server.url(),
            "directory": "/repos/wisp",
            "project_path": "/repos/wisp",
            "updated_at": now_millis() + 60_000,
            "session_id": "ses_future",
            "session_activity": "idle",
            "session_waiting": { "permissions": 0, "questions": 0 }
        })
        .to_string(),
    )
    .unwrap();

    let status = live_status(registry.path()).unwrap();

    assert_eq!(status, Default::default());
    assert!(!path.exists());
    assert!(server.requests().is_empty());
}

#[test]
fn duplicate_live_session_ids_from_different_servers_are_marked_as_conflicts() {
    let duplicate = r#"[{
        "id":"ses_duplicate","projectID":"project","directory":"/repos/wisp",
        "title":"Duplicate task","agent":"build","version":"1.18.15",
        "time":{"created":1,"updated":5}
    }]"#;
    let shared = FakeServer::new(routes(duplicate));
    let unmanaged = FakeServer::new(routes(duplicate));
    let registry = TempDir::new().unwrap();
    fs::write(
        registry.path().join("instance.json"),
        serde_json::json!({
            "registry_version": 3,
            "instance_id": "456:/repos/wisp",
            "pid": 456,
            "server_url": unmanaged.url(),
            "directory": "/repos/wisp",
            "project_path": "/repos/wisp",
            "updated_at": now_millis(),
            "session_activity": null,
            "session_waiting": { "permissions": 0, "questions": 0 }
        })
        .to_string(),
    )
    .unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&shared), registry.path().to_path_buf());

    let snapshot = client.snapshot(Path::new("/repos/wisp")).unwrap();

    assert_eq!(snapshot.sessions.len(), 1);
    assert!(snapshot.conflicts.contains("ses_duplicate"));
}

#[test]
fn plugin_registration_is_versioned_atomic_updatable_and_removable() {
    let registry = TempDir::new().unwrap();
    let registration = RegistryRegistration {
        server_url: "http://127.0.0.1:4096".into(),
        directory: Path::new("/repos/wisp/packages/cli").to_path_buf(),
        project_path: Path::new("/repos/wisp").to_path_buf(),
        pid: 123,
        pane_id: Some("42".into()),
        session_id: None,
        session_activity: None,
        session_waiting: Default::default(),
        session_error: None,
    };

    let path = register_instance(registry.path(), &registration).unwrap();
    let first: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(first["registry_version"], 3);
    assert_eq!(first["instance_id"], "123:/repos/wisp/packages/cli");
    assert_eq!(first["pane_id"], "42");
    assert_eq!(first["session_id"], serde_json::Value::Null);

    let mut updated = registration;
    updated.session_id = Some("ses_123".into());
    updated.session_activity = Some(SessionActivity::Running);
    updated.session_waiting.permissions = 2;
    updated.session_error = Some("provider failed".into());
    assert_eq!(register_instance(registry.path(), &updated).unwrap(), path);
    let second: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(second["session_id"], "ses_123");
    assert_eq!(second["session_activity"], "running");
    assert_eq!(second["session_waiting"]["permissions"], 2);
    assert_eq!(second["session_error"], "provider failed");

    unregister_instance(registry.path(), updated.pid, &updated.directory).unwrap();
    assert!(!path.exists());
}

#[test]
fn plugin_registration_rejects_non_loopback_servers() {
    let registry = TempDir::new().unwrap();
    let error = register_instance(
        registry.path(),
        &RegistryRegistration {
            server_url: "http://example.com:4096".into(),
            directory: PathBuf::from("/repos/wisp"),
            project_path: PathBuf::from("/repos/wisp"),
            pid: 123,
            pane_id: None,
            session_id: None,
            session_activity: None,
            session_waiting: Default::default(),
            session_error: None,
        },
    )
    .unwrap_err();

    assert!(matches!(error, OpenCodeError::InvalidServerUrl(_)));
}

#[test]
fn live_status_counts_fresh_registered_sessions_by_display_state() {
    let server = FakeServer::new(BTreeMap::new());
    let registry = TempDir::new().unwrap();
    for (pid, session_id, session_activity, session_waiting, session_error) in [
        (
            1,
            "ses_waiting",
            SessionActivity::Running,
            SessionWaiting {
                permissions: 1,
                questions: 0,
            },
            None,
        ),
        (
            2,
            "ses_running",
            SessionActivity::Running,
            SessionWaiting::default(),
            None,
        ),
        (
            3,
            "ses_retrying",
            SessionActivity::Retrying {
                attempt: 2,
                message: "rate limited".into(),
                next_at: 99,
            },
            SessionWaiting::default(),
            None,
        ),
        (
            4,
            "ses_idle",
            SessionActivity::Idle,
            SessionWaiting::default(),
            None,
        ),
        (
            5,
            "ses_error",
            SessionActivity::Idle,
            SessionWaiting::default(),
            Some("provider failed"),
        ),
    ] {
        register_instance(
            registry.path(),
            &RegistryRegistration {
                server_url: server.url(),
                directory: PathBuf::from("/repos/wisp"),
                project_path: PathBuf::from("/repos/wisp"),
                pid,
                pane_id: Some(pid.to_string()),
                session_id: Some(session_id.into()),
                session_activity: Some(session_activity),
                session_waiting,
                session_error: session_error.map(str::to_string),
            },
        )
        .unwrap();
    }

    let status = live_status(registry.path()).unwrap();

    assert_eq!(status.waiting, 1);
    assert_eq!(status.running, 1);
    assert_eq!(status.retrying, 1);
    assert_eq!(status.idle, 1);
    assert_eq!(status.error, 1);
    assert!(server.requests().is_empty());
}

#[test]
fn live_status_uses_event_backed_registration_state_without_http() {
    let unavailable = TcpListener::bind("127.0.0.1:0").unwrap();
    let server_url = format!("http://{}", unavailable.local_addr().unwrap());
    drop(unavailable);
    let registry = TempDir::new().unwrap();
    let states = [
        (
            1,
            "ses_waiting",
            serde_json::json!("running"),
            serde_json::json!({ "permissions": 1, "questions": 0 }),
            serde_json::Value::Null,
        ),
        (
            2,
            "ses_running",
            serde_json::json!("running"),
            serde_json::json!({ "permissions": 0, "questions": 0 }),
            serde_json::Value::Null,
        ),
        (
            3,
            "ses_retrying",
            serde_json::json!({
                "retrying": { "attempt": 2, "message": "rate limited", "next_at": 99 }
            }),
            serde_json::json!({ "permissions": 0, "questions": 0 }),
            serde_json::Value::Null,
        ),
        (
            4,
            "ses_idle",
            serde_json::json!("idle"),
            serde_json::json!({ "permissions": 0, "questions": 0 }),
            serde_json::Value::Null,
        ),
        (
            5,
            "ses_error",
            serde_json::json!("idle"),
            serde_json::json!({ "permissions": 0, "questions": 0 }),
            serde_json::json!("provider failed"),
        ),
    ];
    for (pid, session_id, activity, waiting, error) in states {
        fs::write(
            registry.path().join(format!("{pid}.json")),
            serde_json::json!({
                "registry_version": 3,
                "instance_id": format!("{pid}:/repos/wisp"),
                "pid": pid,
                "server_url": server_url,
                "directory": "/repos/wisp",
                "project_path": "/repos/wisp",
                "updated_at": now_millis(),
                "pane_id": pid.to_string(),
                "session_id": session_id,
                "session_activity": activity,
                "session_waiting": waiting,
                "session_error": error,
            })
            .to_string(),
        )
        .unwrap();
    }

    let status = live_status(registry.path()).unwrap();

    assert_eq!(status.waiting, 1);
    assert_eq!(status.running, 1);
    assert_eq!(status.retrying, 1);
    assert_eq!(status.idle, 1);
    assert_eq!(status.error, 1);
}

#[test]
fn live_status_does_not_poll_an_in_process_server_url() {
    let unavailable = TcpListener::bind("127.0.0.1:0").unwrap();
    let server_url = format!("http://{}", unavailable.local_addr().unwrap());
    drop(unavailable);
    let registry = TempDir::new().unwrap();
    register_instance(
        registry.path(),
        &RegistryRegistration {
            server_url,
            directory: PathBuf::from("/repos/wisp"),
            project_path: PathBuf::from("/repos/wisp"),
            pid: 1,
            pane_id: None,
            session_id: Some("ses_unreachable".into()),
            session_activity: Some(SessionActivity::Running),
            session_waiting: Default::default(),
            session_error: None,
        },
    )
    .unwrap();

    let status = live_status(registry.path()).unwrap();

    assert_eq!(status.running, 1);
    assert_eq!(status.error, 0);
}

#[test]
fn live_status_counts_a_launch_before_opencode_exposes_its_session() {
    let registry = TempDir::new().unwrap();
    register_instance(
        registry.path(),
        &RegistryRegistration {
            server_url: "http://localhost:4096".into(),
            directory: PathBuf::from("/repos/wisp"),
            project_path: PathBuf::from("/repos/wisp"),
            pid: 1,
            pane_id: Some("42".into()),
            session_id: None,
            session_activity: None,
            session_waiting: Default::default(),
            session_error: None,
        },
    )
    .unwrap();

    let status = live_status(registry.path()).unwrap();

    assert_eq!(status.idle, 1);
    assert_eq!(
        status.waiting + status.running + status.retrying + status.error,
        0
    );
}

#[test]
fn shared_server_watcher_reports_relevant_sse_events() {
    let server = FakeServer::new(BTreeMap::from([
        ("/global/health", r#"{"healthy":true,"version":"1.18.15"}"#),
        (
            "/global/event",
            "data: {\"directory\":\"/repos/wisp\",\"payload\":{\"type\":\"session.status\",\"properties\":{\"sessionID\":\"ses_123\",\"status\":{\"type\":\"busy\"}}}}\n\n",
        ),
    ]));
    let registry = TempDir::new().unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&server), registry.path().to_path_buf());

    let watcher = client.watch_shared();

    assert!(watcher.changed_timeout(Duration::from_secs(2)));
    assert!(
        server
            .requests()
            .iter()
            .any(|request| request == "/global/event")
    );
}

#[test]
fn shared_server_error_events_are_reflected_in_the_next_snapshot() {
    let mut server_routes = routes(
        r#"[{
            "id":"ses_error","projectID":"project","directory":"/repos/wisp",
            "title":"Failed task","agent":"build","version":"1.18.15",
            "time":{"created":1,"updated":5}
        }]"#,
    );
    server_routes.insert(
        "/global/event",
        "data: {\"payload\":{\"type\":\"session.error\",\"properties\":{\"sessionID\":\"ses_error\",\"error\":{\"name\":\"UnknownError\",\"data\":{\"message\":\"provider failed\"}}}}}\n\n",
    );
    let server = FakeServer::new(server_routes);
    let registry = TempDir::new().unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&server), registry.path().to_path_buf());
    let watcher = client.watch_shared();
    assert!(watcher.changed_timeout(Duration::from_secs(2)));

    let snapshot = client.snapshot(Path::new("/repos/wisp")).unwrap();

    assert_eq!(
        snapshot.sessions[0].activity,
        SessionActivity::Error {
            message: "provider failed".into(),
        }
    );
}

#[test]
fn a_later_running_event_clears_a_retained_session_error() {
    let mut server_routes = routes(
        r#"[{
            "id":"ses_recovered","projectID":"project","directory":"/repos/wisp",
            "title":"Recovered task","agent":"build","version":"1.18.15",
            "time":{"created":1,"updated":5}
        }]"#,
    );
    server_routes.insert(
        "/global/event",
        concat!(
            "data: {\"payload\":{\"type\":\"session.error\",\"properties\":{\"sessionID\":\"ses_recovered\",\"error\":{\"name\":\"UnknownError\",\"data\":{\"message\":\"provider failed\"}}}}}\n\n",
            "data: {\"payload\":{\"type\":\"session.status\",\"properties\":{\"sessionID\":\"ses_recovered\",\"status\":{\"type\":\"busy\"}}}}\n\n"
        ),
    );
    let server = FakeServer::new(server_routes);
    let registry = TempDir::new().unwrap();
    let client = OpenCodeClient::with_registry_dir(config(&server), registry.path().to_path_buf());
    let watcher = client.watch_shared();
    assert!(watcher.changed_timeout(Duration::from_secs(2)));
    thread::sleep(Duration::from_millis(50));

    let snapshot = client.snapshot(Path::new("/repos/wisp")).unwrap();

    assert_eq!(snapshot.sessions[0].activity, SessionActivity::Idle);
}

#[test]
fn shared_server_watcher_keeps_idle_streams_open_until_an_event_arrives() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let server = thread::spawn(move || {
        let (mut health, _) = listener.accept().unwrap();
        read_request(&mut health);
        let body = r#"{"healthy":true,"version":"1.18.15"}"#;
        write!(
            health,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();

        let (mut events, _) = listener.accept().unwrap();
        read_request(&mut events);
        write!(
            events,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        events.flush().unwrap();
        thread::sleep(Duration::from_millis(2_500));
        let _ = write!(
            events,
            "data: {{\"payload\":{{\"type\":\"session.status\",\"properties\":{{}}}}}}\n\n"
        );
        let _ = events.flush();
    });
    let registry = TempDir::new().unwrap();
    let client = OpenCodeClient::with_registry_dir(
        OpenCodeConfig {
            server_url: format!("http://{address}"),
            command: vec!["opencode".into()],
            session_limit: 100,
        },
        registry.path().to_path_buf(),
    );

    let watcher = client.watch_shared();

    assert!(watcher.changed_timeout(Duration::from_secs(4)));
    drop(watcher);
    server.join().unwrap();
}

fn read_request(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
}
