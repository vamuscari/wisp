use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::model::Project;

pub const PROTOCOL_VERSION: u32 = 2;

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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostProjectContext {
    pub labels: Vec<String>,
    #[serde(default)]
    pub items: Vec<HostItem>,
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
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawHostContext {
            protocol_version: u32,
            projects: BTreeMap<String, HostProjectContext>,
        }

        let raw = RawHostContext::deserialize(deserializer)?;
        if raw.protocol_version != PROTOCOL_VERSION {
            return Err(de::Error::custom(HostContextError::UnsupportedVersion(
                raw.protocol_version,
            )));
        }
        Self::new(raw.projects).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionStatus {
    Selected,
    Cancelled,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
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
}
