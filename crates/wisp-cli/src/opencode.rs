use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, TryRecvError},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use directories::BaseDirs;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use thiserror::Error;
use wisp_core::{
    config::OpenCodeConfig,
    opencode::{
        OpenCodeSession, OpenCodeSnapshot, OpenCodeStatusCounts, SessionActivity,
        SessionDisplayState, SessionWaiting,
    },
    path::comparison_key,
    protocol::PROTOCOL_VERSION,
};

pub const SUPPORTED_OPENCODE_VERSION: &str = "1.18.15";
const REGISTRY_VERSION: u32 = PROTOCOL_VERSION;
const REGISTRY_STALE_AFTER_MILLIS: u64 = 90_000;

#[derive(Debug, Error)]
pub enum OpenCodeError {
    #[error("could not determine the platform data directory")]
    MissingDataDirectory,
    #[error("OpenCode request to {url} failed: {message}")]
    Http { url: String, message: String },
    #[error("OpenCode returned invalid JSON from {url}: {source}")]
    Decode {
        url: String,
        source: serde_json::Error,
    },
    #[error("unsupported OpenCode server version {found}; expected {SUPPORTED_OPENCODE_VERSION}")]
    UnsupportedVersion { found: String },
    #[error("OpenCode registry I/O failed at {path}: {source}")]
    RegistryIo { path: PathBuf, source: io::Error },
    #[error("OpenCode registry rejected server URL {0}; expected a loopback HTTP URL")]
    InvalidServerUrl(String),
    #[error("invalid OpenCode registry entry: {0}")]
    InvalidRegistration(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryRegistration {
    pub server_url: String,
    pub directory: PathBuf,
    pub project_path: PathBuf,
    pub pid: u32,
    pub pane_id: Option<String>,
    pub session_id: Option<String>,
    pub session_activity: Option<SessionActivity>,
    pub session_waiting: SessionWaiting,
    pub session_error: Option<String>,
}

pub fn register_instance(
    registry_dir: &Path,
    registration: &RegistryRegistration,
) -> Result<PathBuf, OpenCodeError> {
    if !is_loopback_http_url(&registration.server_url) {
        return Err(OpenCodeError::InvalidServerUrl(
            registration.server_url.clone(),
        ));
    }
    if registration.pid == 0 {
        return Err(OpenCodeError::InvalidRegistration(
            "pid must be greater than zero".into(),
        ));
    }
    if registration.directory.as_os_str().is_empty()
        || registration.project_path.as_os_str().is_empty()
    {
        return Err(OpenCodeError::InvalidRegistration(
            "directory and project_path must not be empty".into(),
        ));
    }
    if registration.pane_id.as_deref() == Some("")
        || registration.session_id.as_deref() == Some("")
        || registration.session_error.as_deref() == Some("")
    {
        return Err(OpenCodeError::InvalidRegistration(
            "pane_id, session_id, and session_error must not be empty".into(),
        ));
    }
    if registration.session_error.is_some() && registration.session_id.is_none() {
        return Err(OpenCodeError::InvalidRegistration(
            "session_error requires session_id".into(),
        ));
    }
    if registration.session_id.is_some() != registration.session_activity.is_some() {
        return Err(OpenCodeError::InvalidRegistration(
            "session_id and session_activity must be supplied together".into(),
        ));
    }
    if registration.session_id.is_none()
        && registration.session_waiting != SessionWaiting::default()
    {
        return Err(OpenCodeError::InvalidRegistration(
            "session_waiting requires session_id".into(),
        ));
    }
    if matches!(
        registration.session_activity.as_ref(),
        Some(SessionActivity::Error { .. })
    ) {
        return Err(OpenCodeError::InvalidRegistration(
            "session_activity must be an OpenCode status".into(),
        ));
    }

    fs::create_dir_all(registry_dir).map_err(|source| OpenCodeError::RegistryIo {
        path: registry_dir.to_path_buf(),
        source,
    })?;
    let instance_id = instance_id(registration.pid, &registration.directory);
    let path = registry_path(registry_dir, &instance_id);
    let document = RegistryDocument {
        registry_version: REGISTRY_VERSION,
        instance_id,
        pid: registration.pid,
        server_url: &registration.server_url,
        directory: &registration.directory,
        project_path: &registration.project_path,
        updated_at: now_millis(),
        pane_id: registration.pane_id.as_deref(),
        session_id: registration.session_id.as_deref(),
        session_activity: registration.session_activity.as_ref(),
        session_waiting: &registration.session_waiting,
        session_error: registration.session_error.as_deref(),
    };
    let mut temporary =
        NamedTempFile::new_in(registry_dir).map_err(|source| OpenCodeError::RegistryIo {
            path: registry_dir.to_path_buf(),
            source,
        })?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        serde_json::to_writer_pretty(&mut writer, &document).map_err(|source| {
            OpenCodeError::RegistryIo {
                path: path.clone(),
                source: io::Error::new(io::ErrorKind::InvalidData, source),
            }
        })?;
        writer
            .write_all(b"\n")
            .map_err(|source| OpenCodeError::RegistryIo {
                path: path.clone(),
                source,
            })?;
        writer.flush().map_err(|source| OpenCodeError::RegistryIo {
            path: path.clone(),
            source,
        })?;
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|source| OpenCodeError::RegistryIo {
            path: path.clone(),
            source,
        })?;
    temporary
        .persist(&path)
        .map_err(|error| OpenCodeError::RegistryIo {
            path: path.clone(),
            source: error.error,
        })?;
    Ok(path)
}

pub fn unregister_instance(
    registry_dir: &Path,
    pid: u32,
    directory: &Path,
) -> Result<(), OpenCodeError> {
    let path = registry_path(registry_dir, &instance_id(pid, directory));
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(OpenCodeError::RegistryIo { path, source }),
    }
}

pub fn live_status(registry_dir: &Path) -> Result<OpenCodeStatusCounts, OpenCodeError> {
    let mut sessions = BTreeMap::<String, OpenCodeSession>::new();
    for registration in registry_entries(registry_dir)? {
        let session_id = registration
            .session_id
            .unwrap_or_else(|| format!("launch:{}", registration.instance_id));
        let mut activity = registration
            .session_activity
            .unwrap_or(SessionActivity::Idle);
        if matches!(activity, SessionActivity::Idle) {
            if let Some(message) = registration.session_error {
                activity = SessionActivity::Error { message };
            }
        }
        let session = OpenCodeSession {
            id: session_id.clone(),
            title: String::new(),
            directory: registration.directory,
            server_url: registration.server_url,
            agent: None,
            parent_id: None,
            updated_at: registration.updated_at,
            activity,
            waiting: registration.session_waiting,
        };
        if let Some(existing) = sessions.get_mut(&session_id) {
            if existing.server_url != session.server_url {
                existing.activity = SessionActivity::Error {
                    message: "session ID is registered by multiple OpenCode servers".into(),
                };
                existing.waiting = SessionWaiting::default();
            } else if display_state_rank(&session) > display_state_rank(existing) {
                *existing = session;
            }
        } else {
            sessions.insert(session_id, session);
        }
    }
    Ok(OpenCodeStatusCounts::from_sessions(
        &sessions.into_values().collect::<Vec<_>>(),
    ))
}

fn display_state_rank(session: &OpenCodeSession) -> u8 {
    match session.display_state() {
        SessionDisplayState::Waiting { .. } => 5,
        SessionDisplayState::Retrying { .. } => 4,
        SessionDisplayState::Running => 3,
        SessionDisplayState::Error { .. } => 2,
        SessionDisplayState::Idle => 1,
    }
}

#[derive(Clone)]
pub struct OpenCodeClient {
    config: OpenCodeConfig,
    registry_dir: PathBuf,
    agent: ureq::Agent,
    auth: Option<(String, String)>,
    event_errors: Arc<Mutex<BTreeMap<String, String>>>,
}

pub struct OpenCodeWatcher {
    receiver: Receiver<()>,
    stop: Arc<AtomicBool>,
}

impl OpenCodeWatcher {
    pub fn changed(&self) -> bool {
        match self.receiver.try_recv() {
            Ok(()) => true,
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => false,
        }
    }

    pub fn changed_timeout(&self, timeout: Duration) -> bool {
        match self.receiver.recv_timeout(timeout) {
            Ok(()) => true,
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => false,
        }
    }
}

impl Drop for OpenCodeWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl OpenCodeClient {
    pub fn new(config: OpenCodeConfig) -> Result<Self, OpenCodeError> {
        let registry_dir = registry_dir()?;
        Ok(Self::with_registry_dir(config, registry_dir))
    }

    pub fn with_registry_dir(config: OpenCodeConfig, registry_dir: PathBuf) -> Self {
        let password = env::var("OPENCODE_SERVER_PASSWORD")
            .ok()
            .filter(|value| !value.is_empty());
        let auth = password.map(|password| {
            let username = env::var("OPENCODE_SERVER_USERNAME")
                .ok()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "opencode".into());
            (username, password)
        });
        let agent = http_agent(Duration::from_secs(2));
        Self {
            config,
            registry_dir,
            agent,
            auth,
            event_errors: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn snapshot(&self, project_path: &Path) -> Result<OpenCodeSnapshot, OpenCodeError> {
        let mut sources = BTreeMap::new();
        sources.insert(
            self.config.server_url.clone(),
            Source {
                server_url: self.config.server_url.clone(),
                directory: project_path.to_path_buf(),
                shared: true,
                host_items: BTreeMap::new(),
                session_errors: BTreeMap::new(),
            },
        );
        for registered in self.registry_sources(project_path)? {
            let source = sources
                .entry(registered.server_url.clone())
                .or_insert_with(|| Source {
                    server_url: registered.server_url.clone(),
                    directory: registered.directory.clone(),
                    shared: false,
                    host_items: BTreeMap::new(),
                    session_errors: BTreeMap::new(),
                });
            if let Some(session_id) = &registered.session_id {
                if let Some(pane_id) = &registered.pane_id {
                    source
                        .host_items
                        .insert(session_id.clone(), format!("pane:{pane_id}"));
                }
                if let Some(error) = registered.session_error {
                    source.session_errors.insert(session_id.clone(), error);
                }
            }
        }

        let mut snapshot = OpenCodeSnapshot::default();
        let mut sessions = BTreeMap::<String, OpenCodeSession>::new();
        let mut successful_sources = 0;
        let mut shared_error = None;
        for source in sources.values() {
            match self.source_snapshot(source) {
                Ok(source_sessions) => {
                    successful_sources += 1;
                    let source_ids = source_sessions
                        .iter()
                        .map(|session| session.id.as_str())
                        .collect::<BTreeSet<_>>();
                    for (session_id, pane_id) in &source.host_items {
                        if source_ids.contains(session_id.as_str()) {
                            snapshot
                                .host_items
                                .insert(session_id.clone(), pane_id.clone());
                        }
                    }
                    for session in source_sessions {
                        if let Some(existing) = sessions.get(&session.id) {
                            if existing.server_url != session.server_url {
                                snapshot.conflicts.insert(session.id.clone());
                            }
                            continue;
                        }
                        sessions.insert(session.id.clone(), session);
                    }
                }
                Err(error) => {
                    if source.shared {
                        shared_error = Some(error);
                    }
                }
            }
        }
        if successful_sources == 0 {
            return Err(shared_error.expect("the configured shared source is always present"));
        }
        snapshot.sessions = sessions.into_values().collect();
        snapshot
            .sessions
            .sort_by_key(|session| std::cmp::Reverse(session.updated_at));
        Ok(snapshot)
    }

    pub fn watch_shared(&self) -> OpenCodeWatcher {
        let (sender, receiver) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let server_url = self.config.server_url.clone();
        let agent = http_agent(Duration::from_secs(15));
        let auth = self.auth.clone();
        let event_errors = Arc::clone(&self.event_errors);
        thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                let health_url = format!("{}/global/health", server_url.trim_end_matches('/'));
                let mut health_request = agent.get(&health_url);
                if let Some((username, password)) = &auth {
                    let encoded = STANDARD.encode(format!("{username}:{password}"));
                    health_request =
                        health_request.set("Authorization", &format!("Basic {encoded}"));
                }
                let healthy = health_request.call().ok().is_some_and(|response| {
                    let mut encoded = Vec::new();
                    response.into_reader().read_to_end(&mut encoded).is_ok()
                        && decode_health(&health_url, &encoded).is_ok_and(|health| health.healthy)
                });
                if !healthy {
                    thread::sleep(Duration::from_millis(250));
                    continue;
                }

                let event_url = format!("{}/global/event", server_url.trim_end_matches('/'));
                let mut event_request = agent.get(&event_url);
                if let Some((username, password)) = &auth {
                    let encoded = STANDARD.encode(format!("{username}:{password}"));
                    event_request = event_request.set("Authorization", &format!("Basic {encoded}"));
                }
                let Ok(response) = event_request.call() else {
                    thread::sleep(Duration::from_millis(250));
                    continue;
                };
                for line in BufReader::new(response.into_reader()).lines() {
                    if thread_stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let Ok(line) = line else {
                        break;
                    };
                    let Some(encoded) = line.strip_prefix("data:") else {
                        continue;
                    };
                    let Ok(event) = serde_json::from_str::<GlobalEvent>(encoded.trim()) else {
                        continue;
                    };
                    if relevant_event(&event.payload.event_type) {
                        update_session_errors(&event.payload, &event_errors);
                        let _ = sender.try_send(());
                    }
                }
                thread::sleep(Duration::from_millis(50));
            }
        });
        OpenCodeWatcher { receiver, stop }
    }

    fn source_snapshot(&self, source: &Source) -> Result<Vec<OpenCodeSession>, OpenCodeError> {
        let health = self.get_health(source)?;
        if !health.healthy {
            return Err(OpenCodeError::Http {
                url: format!("{}/global/health", source.server_url),
                message: "server reported unhealthy status".into(),
            });
        }
        let directory = source.directory.to_string_lossy().into_owned();
        let limit = self.config.session_limit.to_string();
        let raw_sessions: Vec<RawSession> = self.get_json(
            source,
            "/session",
            &[
                ("directory", directory.as_str()),
                ("scope", "project"),
                ("limit", limit.as_str()),
            ],
        )?;
        let statuses: BTreeMap<String, RawStatus> = self.get_json(
            source,
            "/session/status",
            &[("directory", directory.as_str())],
        )?;
        let permissions: Vec<PendingRequest> =
            self.get_json(source, "/permission", &[("directory", directory.as_str())])?;
        let questions: Vec<PendingRequest> =
            self.get_json(source, "/question", &[("directory", directory.as_str())])?;

        let mut waiting = BTreeMap::<String, SessionWaiting>::new();
        for request in permissions {
            waiting.entry(request.session_id).or_default().permissions += 1;
        }
        for request in questions {
            waiting.entry(request.session_id).or_default().questions += 1;
        }

        let mut event_errors = source.session_errors.clone();
        if source.shared {
            event_errors.extend(
                self.event_errors
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone(),
            );
        }
        Ok(raw_sessions
            .into_iter()
            .map(|session| {
                let activity = statuses
                    .get(&session.id)
                    .map(SessionActivity::from)
                    .unwrap_or(SessionActivity::Idle);
                let activity = if matches!(activity, SessionActivity::Idle) {
                    event_errors
                        .get(&session.id)
                        .map(|message| SessionActivity::Error {
                            message: message.clone(),
                        })
                        .unwrap_or(activity)
                } else {
                    activity
                };
                OpenCodeSession {
                    activity,
                    waiting: waiting.remove(&session.id).unwrap_or_default(),
                    id: session.id,
                    title: session.title,
                    directory: session.directory,
                    server_url: source.server_url.clone(),
                    agent: session.agent,
                    parent_id: session.parent_id,
                    updated_at: session.time.updated,
                }
            })
            .collect())
    }

    fn get_json<T: DeserializeOwned>(
        &self,
        source: &Source,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, OpenCodeError> {
        let (url, encoded) = self.get_bytes(source, path, query)?;
        serde_json::from_slice(&encoded).map_err(|source| OpenCodeError::Decode { url, source })
    }

    fn get_health(&self, source: &Source) -> Result<Health, OpenCodeError> {
        let (url, encoded) = self.get_bytes(source, "/global/health", &[])?;
        decode_health(&url, &encoded)
    }

    fn get_bytes(
        &self,
        source: &Source,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<(String, Vec<u8>), OpenCodeError> {
        let url = format!("{}{}", source.server_url.trim_end_matches('/'), path);
        let mut request = self.agent.get(&url);
        for (key, value) in query {
            request = request.query(key, value);
        }
        if source.shared {
            if let Some((username, password)) = &self.auth {
                let encoded = STANDARD.encode(format!("{username}:{password}"));
                request = request.set("Authorization", &format!("Basic {encoded}"));
            }
        }
        let response = request.call().map_err(|error| OpenCodeError::Http {
            url: url.clone(),
            message: error.to_string(),
        })?;
        let mut encoded = Vec::new();
        response
            .into_reader()
            .read_to_end(&mut encoded)
            .map_err(|error| OpenCodeError::Http {
                url: url.clone(),
                message: error.to_string(),
            })?;
        Ok((url, encoded))
    }

    fn registry_sources(
        &self,
        project_path: &Path,
    ) -> Result<Vec<RegisteredSource>, OpenCodeError> {
        let project_key = comparison_key(&project_path.to_string_lossy());
        Ok(registry_entries(&self.registry_dir)?
            .into_iter()
            .filter(|registered| {
                comparison_key(&registered.project_path.to_string_lossy()) == project_key
            })
            .map(|registered| RegisteredSource {
                server_url: registered.server_url,
                directory: registered.directory,
                pane_id: registered.pane_id,
                session_id: registered.session_id,
                session_error: registered.session_error,
            })
            .collect())
    }
}

fn registry_entries(registry_dir: &Path) -> Result<Vec<RegistryEntry>, OpenCodeError> {
    let entries = match fs::read_dir(registry_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(OpenCodeError::RegistryIo {
                path: registry_dir.to_path_buf(),
                source,
            });
        }
    };
    let mut registrations = Vec::new();
    let now = now_millis();
    for entry in entries {
        let entry = entry.map_err(|source| OpenCodeError::RegistryIo {
            path: registry_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let encoded = match fs::read(&path) {
            Ok(encoded) => encoded,
            Err(_) => {
                let _ = fs::remove_file(&path);
                continue;
            }
        };
        let version = serde_json::from_slice::<RegistryHeader>(&encoded);
        if !matches!(
            version,
            Ok(RegistryHeader {
                registry_version: REGISTRY_VERSION
            })
        ) {
            let _ = fs::remove_file(&path);
            continue;
        }
        let registered = match serde_json::from_slice::<RegistryEntry>(&encoded) {
            Ok(registered) if valid_registry_entry(&registered, now) => registered,
            _ => {
                let _ = fs::remove_file(&path);
                continue;
            }
        };
        registrations.push(registered);
    }
    Ok(registrations)
}

fn valid_registry_entry(entry: &RegistryEntry, now: u64) -> bool {
    entry.pid > 0
        && entry.instance_id == instance_id(entry.pid, &entry.directory)
        && is_loopback_http_url(&entry.server_url)
        && !entry.directory.as_os_str().is_empty()
        && !entry.project_path.as_os_str().is_empty()
        && entry.pane_id.as_deref() != Some("")
        && entry.session_id.as_deref() != Some("")
        && entry.session_error.as_deref() != Some("")
        && entry.session_id.is_some() == entry.session_activity.is_some()
        && (entry.session_id.is_some() || entry.session_waiting == SessionWaiting::default())
        && !matches!(
            entry.session_activity.as_ref(),
            Some(SessionActivity::Error { .. })
        )
        && (entry.session_error.is_none() || entry.session_id.is_some())
        && entry.updated_at <= now
        && now - entry.updated_at <= REGISTRY_STALE_AFTER_MILLIS
}

#[derive(Debug)]
struct Source {
    server_url: String,
    directory: PathBuf,
    shared: bool,
    host_items: BTreeMap<String, String>,
    session_errors: BTreeMap<String, String>,
}

#[derive(Debug)]
struct RegisteredSource {
    server_url: String,
    directory: PathBuf,
    pane_id: Option<String>,
    session_id: Option<String>,
    session_error: Option<String>,
}

#[derive(Deserialize)]
struct RegistryHeader {
    registry_version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryEntry {
    #[serde(rename = "registry_version")]
    _registry_version: u32,
    instance_id: String,
    pid: u32,
    server_url: String,
    directory: PathBuf,
    project_path: PathBuf,
    updated_at: u64,
    pane_id: Option<String>,
    session_id: Option<String>,
    session_activity: Option<SessionActivity>,
    session_waiting: SessionWaiting,
    session_error: Option<String>,
}

#[derive(Serialize)]
struct RegistryDocument<'a> {
    registry_version: u32,
    instance_id: String,
    pid: u32,
    server_url: &'a str,
    directory: &'a Path,
    project_path: &'a Path,
    updated_at: u64,
    pane_id: Option<&'a str>,
    session_id: Option<&'a str>,
    session_activity: Option<&'a SessionActivity>,
    session_waiting: &'a SessionWaiting,
    session_error: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Health {
    healthy: bool,
    #[serde(rename = "version")]
    _version: String,
}

#[derive(Deserialize)]
struct HealthVersionHeader {
    version: String,
}

#[derive(Deserialize)]
struct GlobalEvent {
    payload: GlobalEventPayload,
}

#[derive(Deserialize)]
struct GlobalEventPayload {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    properties: serde_json::Value,
}

#[derive(Deserialize)]
struct RawSessionErrorEvent {
    #[serde(rename = "sessionID")]
    session_id: Option<String>,
    error: Option<RawEventError>,
}

#[derive(Deserialize)]
struct RawSessionStatusEvent {
    #[serde(rename = "sessionID")]
    session_id: String,
    status: RawStatus,
}

#[derive(Deserialize)]
struct RawEventError {
    name: String,
    #[serde(default)]
    data: serde_json::Value,
}

#[derive(Deserialize)]
struct RawSession {
    id: String,
    directory: PathBuf,
    #[serde(rename = "parentID")]
    parent_id: Option<String>,
    title: String,
    agent: Option<String>,
    time: RawSessionTime,
}

#[derive(Deserialize)]
struct RawSessionTime {
    updated: u64,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum RawStatus {
    Idle,
    Busy,
    Retry {
        attempt: u32,
        message: String,
        next: u64,
        #[serde(default)]
        action: Option<serde_json::Value>,
    },
}

pub fn decode_session_status(encoded: &str) -> Result<SessionActivity, OpenCodeError> {
    serde_json::from_str::<RawStatus>(encoded)
        .map(|status| SessionActivity::from(&status))
        .map_err(|error| {
            OpenCodeError::InvalidRegistration(format!("invalid session_status: {error}"))
        })
}

impl From<&RawStatus> for SessionActivity {
    fn from(status: &RawStatus) -> Self {
        match status {
            RawStatus::Idle => Self::Idle,
            RawStatus::Busy => Self::Running,
            RawStatus::Retry {
                attempt,
                message,
                next,
                action,
            } => {
                let _ = action;
                Self::Retrying {
                    attempt: *attempt,
                    message: message.clone(),
                    next_at: *next,
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct PendingRequest {
    #[serde(rename = "sessionID")]
    session_id: String,
}

fn registry_dir() -> Result<PathBuf, OpenCodeError> {
    if let Some(path) = env::var_os("WISP_OPENCODE_REGISTRY_DIR").filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(path));
    }
    BaseDirs::new()
        .map(|base| base.data_local_dir().join("wisp/opencode"))
        .ok_or(OpenCodeError::MissingDataDirectory)
}

pub fn default_registry_dir() -> Result<PathBuf, OpenCodeError> {
    registry_dir()
}

fn instance_id(pid: u32, directory: &Path) -> String {
    format!("{pid}:{}", directory.to_string_lossy())
}

fn registry_path(registry_dir: &Path, instance_id: &str) -> PathBuf {
    registry_dir.join(format!(
        "{}.json",
        blake3::hash(instance_id.as_bytes()).to_hex()
    ))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn http_agent(read_timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(500))
        .timeout_read(read_timeout)
        .timeout_write(Duration::from_secs(2))
        .build()
}

fn decode_health(url: &str, encoded: &[u8]) -> Result<Health, OpenCodeError> {
    let header: HealthVersionHeader =
        serde_json::from_slice(encoded).map_err(|source| OpenCodeError::Decode {
            url: url.to_owned(),
            source,
        })?;
    if header.version != SUPPORTED_OPENCODE_VERSION {
        return Err(OpenCodeError::UnsupportedVersion {
            found: header.version,
        });
    }
    serde_json::from_slice(encoded).map_err(|source| OpenCodeError::Decode {
        url: url.to_owned(),
        source,
    })
}

fn relevant_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "session.created"
            | "session.updated"
            | "session.deleted"
            | "session.status"
            | "session.idle"
            | "session.error"
            | "permission.asked"
            | "permission.replied"
            | "permission.v2.asked"
            | "permission.v2.replied"
            | "question.asked"
            | "question.replied"
            | "question.rejected"
            | "question.v2.asked"
            | "question.v2.replied"
            | "question.v2.rejected"
            | "server.instance.disposed"
    )
}

fn update_session_errors(payload: &GlobalEventPayload, errors: &Mutex<BTreeMap<String, String>>) {
    match payload.event_type.as_str() {
        "session.error" => {
            let Ok(event) =
                serde_json::from_value::<RawSessionErrorEvent>(payload.properties.clone())
            else {
                return;
            };
            let (Some(session_id), Some(error)) = (event.session_id, event.error) else {
                return;
            };
            let message = error
                .data
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&error.name)
                .to_owned();
            errors
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(session_id, message);
        }
        "session.status" => {
            let Ok(event) =
                serde_json::from_value::<RawSessionStatusEvent>(payload.properties.clone())
            else {
                return;
            };
            if matches!(event.status, RawStatus::Busy | RawStatus::Retry { .. }) {
                errors
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&event.session_id);
            }
        }
        _ => {}
    }
}

fn is_loopback_http_url(value: &str) -> bool {
    let Some(authority) = value.strip_prefix("http://") else {
        return false;
    };
    let authority = authority.trim_end_matches('/');
    if authority.is_empty()
        || authority.contains('/')
        || authority.contains('?')
        || authority.contains('#')
        || authority.contains('@')
    {
        return false;
    }
    let host = if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, remainder)) = rest.split_once(']') else {
            return false;
        };
        if !remainder.is_empty()
            && !remainder
                .strip_prefix(':')
                .is_some_and(|port| port.parse::<u16>().is_ok_and(|port| port > 0))
        {
            return false;
        }
        host
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if !port.parse::<u16>().is_ok_and(|port| port > 0) {
            return false;
        }
        host
    } else {
        authority
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use std::net::{TcpListener, TcpStream};

    use super::*;

    struct FakeServer {
        address: String,
        requests: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl FakeServer {
        fn new() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap().to_string();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let request_log = Arc::clone(&requests);
            let stop = Arc::new(AtomicBool::new(false));
            let thread_stop = Arc::clone(&stop);
            let thread = thread::spawn(move || {
                while !thread_stop.load(Ordering::Relaxed) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(value) => value,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
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
                        .unwrap();
                    let body = if target.starts_with("/global/health") {
                        r#"{"healthy":true,"version":"1.18.15"}"#
                    } else if target.starts_with("/session/status") {
                        "{}"
                    } else {
                        "[]"
                    };
                    request_log.lock().unwrap().push(request);
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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

    fn source(server: &FakeServer, shared: bool) -> Source {
        Source {
            server_url: server.url(),
            directory: PathBuf::from("/repos/wisp"),
            shared,
            host_items: BTreeMap::new(),
            session_errors: BTreeMap::new(),
        }
    }

    fn has_authorization(request: &str, expected: &str) -> bool {
        request.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.eq_ignore_ascii_case("authorization") && value.trim() == expected
            })
        })
    }

    #[test]
    fn basic_auth_is_only_sent_to_the_configured_shared_source() {
        let shared_server = FakeServer::new();
        let registered_server = FakeServer::new();
        let client = OpenCodeClient {
            config: OpenCodeConfig {
                server_url: shared_server.url(),
                command: vec!["opencode".into()],
                session_limit: 100,
            },
            registry_dir: PathBuf::new(),
            agent: http_agent(Duration::from_secs(2)),
            auth: Some(("wisp".into(), "secret".into())),
            event_errors: Arc::new(Mutex::new(BTreeMap::new())),
        };

        client
            .source_snapshot(&source(&shared_server, true))
            .unwrap();
        client
            .source_snapshot(&source(&registered_server, false))
            .unwrap();

        let expected = format!("Basic {}", STANDARD.encode("wisp:secret"));
        assert!(
            shared_server
                .requests()
                .iter()
                .all(|request| has_authorization(request, &expected))
        );
        assert!(
            registered_server
                .requests()
                .iter()
                .all(|request| !has_authorization(request, &expected))
        );
    }
}
