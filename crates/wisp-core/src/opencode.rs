use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionActivity {
    Idle,
    Running,
    Retrying {
        attempt: u32,
        message: String,
        next_at: u64,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionWaiting {
    pub permissions: usize,
    pub questions: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionDisplayState {
    Waiting {
        permissions: usize,
        questions: usize,
    },
    Retrying {
        attempt: u32,
        message: String,
        next_at: u64,
    },
    Running,
    Idle,
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenCodeSession {
    pub id: String,
    pub title: String,
    pub directory: PathBuf,
    pub server_url: String,
    pub agent: Option<String>,
    pub parent_id: Option<String>,
    pub updated_at: u64,
    pub activity: SessionActivity,
    pub waiting: SessionWaiting,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenCodeSnapshot {
    pub sessions: Vec<OpenCodeSession>,
    pub host_items: BTreeMap<String, String>,
    pub conflicts: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenCodeStatusCounts {
    pub waiting: usize,
    pub running: usize,
    pub retrying: usize,
    pub idle: usize,
    pub error: usize,
}

impl OpenCodeStatusCounts {
    pub fn from_sessions(sessions: &[OpenCodeSession]) -> Self {
        let mut counts = Self::default();
        for session in sessions {
            match session.display_state() {
                SessionDisplayState::Waiting { .. } => counts.waiting += 1,
                SessionDisplayState::Retrying { .. } => counts.retrying += 1,
                SessionDisplayState::Running => counts.running += 1,
                SessionDisplayState::Idle => counts.idle += 1,
                SessionDisplayState::Error { .. } => counts.error += 1,
            }
        }
        counts
    }
}

impl OpenCodeSession {
    pub fn display_state(&self) -> SessionDisplayState {
        if self.waiting.permissions > 0 || self.waiting.questions > 0 {
            return SessionDisplayState::Waiting {
                permissions: self.waiting.permissions,
                questions: self.waiting.questions,
            };
        }

        match &self.activity {
            SessionActivity::Idle => SessionDisplayState::Idle,
            SessionActivity::Running => SessionDisplayState::Running,
            SessionActivity::Retrying {
                attempt,
                message,
                next_at,
            } => SessionDisplayState::Retrying {
                attempt: *attempt,
                message: message.clone(),
                next_at: *next_at,
            },
            SessionActivity::Error { message } => SessionDisplayState::Error {
                message: message.clone(),
            },
        }
    }

    pub fn type_label(&self) -> String {
        format!(
            "{} · {}",
            self.agent.as_deref().unwrap_or("unassigned"),
            if self.parent_id.is_some() {
                "child"
            } else {
                "root"
            }
        )
    }
}
