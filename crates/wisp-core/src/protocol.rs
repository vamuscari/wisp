use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::value::RawValue;
use thiserror::Error;

use crate::{model::Project, opencode::OpenCodeStatusCounts};

pub const PROTOCOL_VERSION: u32 = 3;

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
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HostContextError {
    #[error("unsupported host context version {0}")]
    UnsupportedVersion(u32),
    #[error("host context contains an empty project ID")]
    EmptyProjectId,
    #[error("host context labels must not be empty")]
    EmptyLabel,
    #[error("host item IDs must not be empty")]
    EmptyItemId,
    #[error("host item labels must not be empty")]
    EmptyItemLabel,
    #[error("host context project {project_id} contains duplicate item ID {item_id}")]
    DuplicateItemId { project_id: String, item_id: String },
    #[error("OpenCode session IDs in host context must not be empty")]
    EmptySessionId,
    #[error("OpenCode session host item IDs must not be empty")]
    EmptySessionItemId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostProjectContext {
    pub labels: Vec<String>,
    #[serde(default)]
    pub items: Vec<HostItem>,
    #[serde(default)]
    pub session_items: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostItem {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub active: bool,
}

impl HostContext {
    pub fn new(projects: BTreeMap<String, HostProjectContext>) -> Result<Self, HostContextError> {
        if projects.keys().any(String::is_empty) {
            return Err(HostContextError::EmptyProjectId);
        }
        for (project_id, context) in &projects {
            if context.labels.iter().any(String::is_empty) {
                return Err(HostContextError::EmptyLabel);
            }
            let mut item_ids = BTreeSet::new();
            for item in &context.items {
                if item.id.is_empty() {
                    return Err(HostContextError::EmptyItemId);
                }
                if item.label.is_empty() {
                    return Err(HostContextError::EmptyItemLabel);
                }
                if !item_ids.insert(&item.id) {
                    return Err(HostContextError::DuplicateItemId {
                        project_id: project_id.clone(),
                        item_id: item.id.clone(),
                    });
                }
            }
            if context.session_items.keys().any(String::is_empty) {
                return Err(HostContextError::EmptySessionId);
            }
            if context.session_items.values().any(String::is_empty) {
                return Err(HostContextError::EmptySessionItemId);
            }
        }
        Ok(Self {
            protocol_version: PROTOCOL_VERSION,
            projects,
        })
    }

    pub fn labels(&self, project_id: &str) -> &[String] {
        self.projects
            .get(project_id)
            .map(|context| context.labels.as_slice())
            .unwrap_or_default()
    }

    pub fn items(&self, project_id: &str) -> &[HostItem] {
        self.projects
            .get(project_id)
            .map(|context| context.items.as_slice())
            .unwrap_or_default()
    }

    pub fn session_item(&self, project_id: &str, session_id: &str) -> Option<&str> {
        self.projects
            .get(project_id)?
            .session_items
            .get(session_id)
            .map(String::as_str)
    }
}

impl Default for HostContext {
    fn default() -> Self {
        Self::new(BTreeMap::new()).expect("empty host context is valid")
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
        }

        let raw: RawHostContext = serde_json::from_str(json.get()).map_err(de::Error::custom)?;
        Self::new(raw.projects).map_err(de::Error::custom)
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
    },
    CloseProject {
        project: Project,
    },
    HostItem {
        project: Project,
        id: String,
    },
    OpenCodeSession {
        project: Project,
        session_id: String,
        opener: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        host_item_id: Option<String>,
    },
}
