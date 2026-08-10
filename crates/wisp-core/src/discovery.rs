use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::{
    config::{Config, ProjectConfig},
    model::{DirectoryEntry, EntryKind, Project},
    path::comparison_key,
};

pub trait FileSystem {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirectoryEntry>>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StdFileSystem;

impl FileSystem for StdFileSystem {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirectoryEntry>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.to_str().is_none() {
                continue;
            }
            let file_type = entry.file_type()?;
            let kind = if file_type.is_dir() {
                EntryKind::Directory
            } else if file_type.is_file() {
                EntryKind::File
            } else if file_type.is_symlink() {
                match fs::metadata(&path) {
                    Ok(metadata) if metadata.is_dir() => EntryKind::SymlinkDirectory,
                    Ok(metadata) if metadata.is_file() => EntryKind::SymlinkFile,
                    Ok(_) => EntryKind::Symlink,
                    Err(_) => EntryKind::Symlink,
                }
            } else {
                EntryKind::Other
            };
            entries.push(DirectoryEntry::new(path, kind));
        }
        Ok(entries)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DiscoveryError {
    #[error("duplicate project id {id} for {first} and {second}")]
    DuplicateProjectId {
        id: String,
        first: String,
        second: String,
    },
}

pub fn discover_projects(
    config: &Config,
    file_system: &impl FileSystem,
) -> Result<Vec<Project>, DiscoveryError> {
    let mut projects = Vec::new();
    let mut seen_paths = HashMap::<String, PathBuf>::new();
    let mut seen_ids = HashMap::<String, PathBuf>::new();

    for project in &config.projects {
        add_project(
            &mut projects,
            &mut seen_paths,
            &mut seen_ids,
            configured_project(project),
        )?;
    }

    for root in &config.roots {
        let Ok(entries) = file_system.read_dir(&root.path) else {
            continue;
        };
        let group = root
            .group
            .clone()
            .unwrap_or_else(|| path_basename(&root.path));
        for entry in entries {
            if !entry.kind.is_directory(config.follow_symlinks) {
                continue;
            }
            let name = path_basename(&entry.path);
            let path_text = entry.path.to_string_lossy().into_owned();
            add_project(
                &mut projects,
                &mut seen_paths,
                &mut seen_ids,
                Project {
                    id: path_text,
                    path: entry.path,
                    group: group.clone(),
                    display_name: name.clone(),
                    name,
                },
            )?;
        }
    }

    projects.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(projects)
}

fn configured_project(project: &ProjectConfig) -> Project {
    let path_text = project.path.to_string_lossy().into_owned();
    let name = project
        .name
        .clone()
        .unwrap_or_else(|| path_basename(&project.path));
    Project {
        id: project.id.clone().unwrap_or_else(|| path_text.clone()),
        path: project.path.clone(),
        group: project
            .group
            .clone()
            .unwrap_or_else(|| "Projects".to_string()),
        display_name: project.display_name.clone().unwrap_or_else(|| name.clone()),
        name,
    }
}

fn add_project(
    projects: &mut Vec<Project>,
    seen_paths: &mut HashMap<String, PathBuf>,
    seen_ids: &mut HashMap<String, PathBuf>,
    project: Project,
) -> Result<(), DiscoveryError> {
    let key = comparison_key(&project.path.to_string_lossy());
    if seen_paths.contains_key(&key) {
        return Ok(());
    }
    if let Some(first) = seen_ids.get(&project.id) {
        return Err(DiscoveryError::DuplicateProjectId {
            id: project.id.clone(),
            first: first.to_string_lossy().into_owned(),
            second: project.path.to_string_lossy().into_owned(),
        });
    }
    seen_paths.insert(key, project.path.clone());
    seen_ids.insert(project.id.clone(), project.path.clone());
    projects.push(project);
    Ok(())
}

fn path_basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            path.to_string_lossy()
                .replace('\\', "/")
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string()
        })
}
