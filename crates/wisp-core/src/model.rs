use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub path: PathBuf,
    pub group: String,
    pub name: String,
    pub display_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Directory,
    File,
    SymlinkDirectory,
    SymlinkFile,
    Symlink,
    Other,
}

impl EntryKind {
    pub fn is_directory(self, follow_symlinks: bool) -> bool {
        self == Self::Directory || (follow_symlinks && self == Self::SymlinkDirectory)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub path: PathBuf,
    pub kind: EntryKind,
}

impl DirectoryEntry {
    pub fn new(path: PathBuf, kind: EntryKind) -> Self {
        Self { path, kind }
    }

    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }
}
