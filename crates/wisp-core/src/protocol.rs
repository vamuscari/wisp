use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::value::RawValue;
use thiserror::Error;

use crate::{model::Project, opencode::OpenCodeStatusCounts};

pub const PROTOCOL_VERSION: u32 = 8;

#[derive(Deserialize)]
struct VersionHeader {
    protocol_version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProjectsEnvelope {
    pub protocol_version: u32,
    pub projects: Vec<Project>,
}

impl ProjectsEnvelope {
    pub fn new(projects: Vec<Project>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            projects,
        }
    }
}

impl<'de> Deserialize<'de> for ProjectsEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json = Box::<RawValue>::deserialize(deserializer)?;
        crate::strict_json::reject_duplicate_fields(json.get().as_bytes())
            .map_err(de::Error::custom)?;
        let header: VersionHeader = serde_json::from_str(json.get()).map_err(de::Error::custom)?;
        if header.protocol_version != PROTOCOL_VERSION {
            return Err(de::Error::custom(format!(
                "unsupported projects protocol version {}",
                header.protocol_version
            )));
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawProjectsEnvelope {
            protocol_version: u32,
            projects: Vec<Project>,
        }

        let raw: RawProjectsEnvelope =
            serde_json::from_str(json.get()).map_err(de::Error::custom)?;
        Ok(Self {
            protocol_version: raw.protocol_version,
            projects: raw.projects,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenCodeStatusEnvelope {
    pub protocol_version: u32,
    pub sessions: OpenCodeStatusCounts,
}

impl OpenCodeStatusEnvelope {
    pub fn new(sessions: OpenCodeStatusCounts) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            sessions,
        }
    }
}

impl<'de> Deserialize<'de> for OpenCodeStatusEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json = Box::<RawValue>::deserialize(deserializer)?;
        crate::strict_json::reject_duplicate_fields(json.get().as_bytes())
            .map_err(de::Error::custom)?;
        let header: VersionHeader = serde_json::from_str(json.get()).map_err(de::Error::custom)?;
        if header.protocol_version != PROTOCOL_VERSION {
            return Err(de::Error::custom(format!(
                "unsupported OpenCode status protocol version {}",
                header.protocol_version
            )));
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawOpenCodeStatusEnvelope {
            protocol_version: u32,
            sessions: OpenCodeStatusCounts,
        }

        let raw: RawOpenCodeStatusEnvelope =
            serde_json::from_str(json.get()).map_err(de::Error::custom)?;
        Ok(Self {
            protocol_version: raw.protocol_version,
            sessions: raw.sessions,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostContext {
    protocol_version: u32,
    projects: BTreeMap<String, HostProjectContext>,
    workspaces: BTreeMap<String, HostWorkspaceContext>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HostContextError {
    #[error("unsupported host context version {0}")]
    UnsupportedVersion(u32),
    #[error("host context contains an empty project ID")]
    EmptyProjectId,
    #[error("host context labels must not be empty")]
    EmptyLabel,
    #[error("host window IDs must not be empty")]
    EmptyWindowId,
    #[error("host window labels must not be empty")]
    EmptyWindowLabel,
    #[error("host pane IDs must not be empty")]
    EmptyPaneId,
    #[error("host pane labels must not be empty")]
    EmptyPaneLabel,
    #[error("host window {window_id} in {owner} must contain at least one pane")]
    EmptyWindowPanes { owner: String, window_id: String },
    #[error("{owner} contains duplicate window ID {window_id}")]
    DuplicateWindowId { owner: String, window_id: String },
    #[error("{owner} contains duplicate pane ID {pane_id}")]
    DuplicatePaneId { owner: String, pane_id: String },
    #[error("OpenCode session IDs in host context must not be empty")]
    EmptySessionId,
    #[error("OpenCode session host item IDs must not be empty")]
    EmptySessionItemId,
    #[error("host workspace names must not be empty")]
    EmptyWorkspaceName,
    #[error("invalid Neovim view: {0}")]
    InvalidNvimView(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostProjectContext {
    pub labels: Vec<String>,
    #[serde(default)]
    pub windows: Vec<HostWindow>,
    #[serde(default)]
    pub session_items: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostWorkspaceContext {
    pub current: bool,
    #[serde(default)]
    pub windows: Vec<HostWindow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostWindow {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub active: bool,
    pub panes: Vec<HostPane>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostPane {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub active: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nvim_views: Vec<NvimView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NvimViewport {
    pub lnum: u64,
    pub col: u64,
    pub coladd: u64,
    pub curswant: u64,
    pub topline: u64,
    pub topfill: u64,
    pub leftcol: u64,
    pub skipcol: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NvimView {
    pub window_id: String,
    pub path: PathBuf,
    pub active: bool,
    pub width: u64,
    pub height: u64,
    pub bottomline: u64,
    pub view: NvimViewport,
}

fn validate_nvim_views(views: &[NvimView]) -> Result<(), String> {
    let mut window_ids = BTreeSet::new();
    for view in views {
        if view.window_id.is_empty() {
            return Err("Neovim window IDs must not be empty".into());
        }
        if !window_ids.insert(&view.window_id) {
            return Err(format!("duplicate Neovim window ID {}", view.window_id));
        }
        if !is_absolute_protocol_path(&view.path) {
            return Err("Neovim view paths must be nonempty and absolute".into());
        }
        if view.width == 0 || view.height == 0 {
            return Err("Neovim view dimensions must be positive".into());
        }
        if view.view.lnum == 0 || view.view.topline == 0 || view.bottomline == 0 {
            return Err("Neovim line positions must be positive".into());
        }
        if view.bottomline < view.view.topline {
            return Err("Neovim bottomline must not precede topline".into());
        }
    }
    Ok(())
}

fn is_absolute_protocol_path(path: &std::path::Path) -> bool {
    let bytes = path.to_string_lossy();
    let bytes = bytes.as_bytes();
    bytes.first() == Some(&b'/')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
        || (bytes.len() >= 2 && bytes[0] == b'\\' && bytes[1] == b'\\')
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NvimPaneStateEnvelope {
    pub protocol_version: u32,
    pub views: Vec<NvimView>,
}

impl<'de> Deserialize<'de> for NvimPaneStateEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json = Box::<RawValue>::deserialize(deserializer)?;
        crate::strict_json::reject_duplicate_fields(json.get().as_bytes())
            .map_err(de::Error::custom)?;
        let header: VersionHeader = serde_json::from_str(json.get()).map_err(de::Error::custom)?;
        if header.protocol_version != PROTOCOL_VERSION {
            return Err(de::Error::custom(format!(
                "unsupported Neovim pane state protocol version {}",
                header.protocol_version
            )));
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawNvimPaneStateEnvelope {
            protocol_version: u32,
            views: Vec<NvimView>,
        }

        let raw: RawNvimPaneStateEnvelope =
            serde_json::from_str(json.get()).map_err(de::Error::custom)?;
        validate_nvim_views(&raw.views).map_err(de::Error::custom)?;
        Ok(Self {
            protocol_version: raw.protocol_version,
            views: raw.views,
        })
    }
}

fn validate_windows(owner: &str, windows: &[HostWindow]) -> Result<(), HostContextError> {
    let mut window_ids = BTreeSet::new();
    let mut pane_ids = BTreeSet::new();
    for window in windows {
        if window.id.is_empty() {
            return Err(HostContextError::EmptyWindowId);
        }
        if window.label.is_empty() {
            return Err(HostContextError::EmptyWindowLabel);
        }
        if !window_ids.insert(&window.id) {
            return Err(HostContextError::DuplicateWindowId {
                owner: owner.to_string(),
                window_id: window.id.clone(),
            });
        }
        if window.panes.is_empty() {
            return Err(HostContextError::EmptyWindowPanes {
                owner: owner.to_string(),
                window_id: window.id.clone(),
            });
        }
        for pane in &window.panes {
            if pane.id.is_empty() {
                return Err(HostContextError::EmptyPaneId);
            }
            if pane.label.is_empty() {
                return Err(HostContextError::EmptyPaneLabel);
            }
            if !pane_ids.insert(&pane.id) {
                return Err(HostContextError::DuplicatePaneId {
                    owner: owner.to_string(),
                    pane_id: pane.id.clone(),
                });
            }
            validate_nvim_views(&pane.nvim_views).map_err(HostContextError::InvalidNvimView)?;
        }
    }
    Ok(())
}

impl HostContext {
    pub fn new(
        projects: BTreeMap<String, HostProjectContext>,
        workspaces: BTreeMap<String, HostWorkspaceContext>,
    ) -> Result<Self, HostContextError> {
        if projects.keys().any(String::is_empty) {
            return Err(HostContextError::EmptyProjectId);
        }
        for (project_id, context) in &projects {
            if context.labels.iter().any(String::is_empty) {
                return Err(HostContextError::EmptyLabel);
            }
            validate_windows(&format!("project {project_id}"), &context.windows)?;
            if context.session_items.keys().any(String::is_empty) {
                return Err(HostContextError::EmptySessionId);
            }
            if context.session_items.values().any(String::is_empty) {
                return Err(HostContextError::EmptySessionItemId);
            }
        }
        for (workspace_name, workspace) in &workspaces {
            if workspace_name.is_empty() {
                return Err(HostContextError::EmptyWorkspaceName);
            }
            validate_windows(&format!("workspace {workspace_name}"), &workspace.windows)?;
        }
        Ok(Self {
            protocol_version: PROTOCOL_VERSION,
            projects,
            workspaces,
        })
    }

    pub fn labels(&self, project_id: &str) -> &[String] {
        self.projects
            .get(project_id)
            .map(|context| context.labels.as_slice())
            .unwrap_or_default()
    }

    pub fn windows(&self, project_id: &str) -> &[HostWindow] {
        self.projects
            .get(project_id)
            .map(|context| context.windows.as_slice())
            .unwrap_or_default()
    }

    pub fn session_item(&self, project_id: &str, session_id: &str) -> Option<&str> {
        self.projects
            .get(project_id)?
            .session_items
            .get(session_id)
            .map(String::as_str)
    }

    pub fn workspaces(&self) -> &BTreeMap<String, HostWorkspaceContext> {
        &self.workspaces
    }
}

impl Default for HostContext {
    fn default() -> Self {
        Self::new(BTreeMap::new(), BTreeMap::new()).expect("empty host context is valid")
    }
}

impl<'de> Deserialize<'de> for HostContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json = Box::<RawValue>::deserialize(deserializer)?;
        crate::strict_json::reject_duplicate_fields(json.get().as_bytes())
            .map_err(de::Error::custom)?;
        let header: VersionHeader = serde_json::from_str(json.get()).map_err(de::Error::custom)?;
        if header.protocol_version != PROTOCOL_VERSION {
            return Err(de::Error::custom(HostContextError::UnsupportedVersion(
                header.protocol_version,
            )));
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawHostContext {
            #[serde(rename = "protocol_version")]
            _protocol_version: u32,
            projects: BTreeMap<String, HostProjectContext>,
            workspaces: BTreeMap<String, HostWorkspaceContext>,
        }

        let raw: RawHostContext = serde_json::from_str(json.get()).map_err(de::Error::custom)?;
        Self::new(raw.projects, raw.workspaces).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SelectionEnvelope {
    pub protocol_version: u32,
    pub status: SelectionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SelectionEnvelope {
    pub fn selected(selection: Selection) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            status: SelectionStatus::Selected,
            selection: Some(selection),
            error: None,
        }
    }

    pub fn cancelled() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            status: SelectionStatus::Cancelled,
            selection: None,
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            status: SelectionStatus::Error,
            selection: None,
            error: Some(message.into()),
        }
    }
}

impl<'de> Deserialize<'de> for SelectionEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json = Box::<RawValue>::deserialize(deserializer)?;
        crate::strict_json::reject_duplicate_fields(json.get().as_bytes())
            .map_err(de::Error::custom)?;
        let header: VersionHeader = serde_json::from_str(json.get()).map_err(de::Error::custom)?;
        if header.protocol_version != PROTOCOL_VERSION {
            return Err(de::Error::custom(format!(
                "unsupported selection protocol version {}",
                header.protocol_version
            )));
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawSelectionEnvelope {
            protocol_version: u32,
            status: SelectionStatus,
            selection: Option<Selection>,
            error: Option<String>,
        }

        let raw: RawSelectionEnvelope =
            serde_json::from_str(json.get()).map_err(de::Error::custom)?;
        let valid = match raw.status {
            SelectionStatus::Selected => raw.selection.is_some() && raw.error.is_none(),
            SelectionStatus::Cancelled => raw.selection.is_none() && raw.error.is_none(),
            SelectionStatus::Error => raw.selection.is_none() && raw.error.is_some(),
        };
        if !valid {
            return Err(de::Error::custom(
                "selection envelope fields do not match its status",
            ));
        }
        Ok(Self {
            protocol_version: raw.protocol_version,
            status: raw.status,
            selection: raw.selection,
            error: raw.error,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStatus {
    Selected,
    Cancelled,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileOpenTarget {
    Window,
    RightPane,
    BottomPane,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FileHostTarget {
    pub window_id: String,
    pub pane_id: String,
}

impl<'de> Deserialize<'de> for FileHostTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawFileHostTarget {
            window_id: String,
            pane_id: String,
        }

        let raw = RawFileHostTarget::deserialize(deserializer)?;
        if raw.window_id.is_empty() || raw.pane_id.is_empty() {
            return Err(de::Error::custom(
                "file host target window and pane IDs must not be empty",
            ));
        }
        Ok(Self {
            window_id: raw.window_id,
            pane_id: raw.pane_id,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Selection {
    Project {
        project: Project,
        #[serde(skip_serializing_if = "Option::is_none")]
        opener: Option<Vec<String>>,
    },
    File {
        project: Project,
        path: PathBuf,
        #[serde(skip_serializing_if = "Option::is_none")]
        opener: Option<Vec<String>>,
        open_target: FileOpenTarget,
        reuse_existing: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        host_target: Option<FileHostTarget>,
    },
    CloseProject {
        project: Project,
    },
    HostPane {
        project: Project,
        window_id: String,
        pane_id: String,
    },
    Workspace {
        workspace: String,
    },
    WorkspacePane {
        workspace: String,
        window_id: String,
        pane_id: String,
    },
    CloseWorkspace {
        workspace: String,
    },
    OpenCodeSession {
        project: Project,
        session_id: String,
        opener: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        host_item_id: Option<String>,
    },
}
