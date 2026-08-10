use std::path::PathBuf;

use thiserror::Error;

use crate::{
    config::Openers,
    model::{DirectoryEntry, Project},
    protocol::Selection,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    Projects,
    Directory {
        project_id: String,
        path: PathBuf,
        ancestors: Vec<PathBuf>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavigationOutcome {
    Continue,
    LoadDirectory { project: Project, path: PathBuf },
    Selected(Selection),
    Cancelled,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NavigationError {
    #[error("unknown project {0}")]
    UnknownProject(String),
    #[error("the current screen does not accept this action")]
    InvalidScreen,
}

#[derive(Clone, Debug)]
pub struct Navigator {
    projects: Vec<Project>,
    follow_symlinks: bool,
    screen: Screen,
}

impl Navigator {
    pub fn new(projects: Vec<Project>, follow_symlinks: bool) -> Self {
        Self {
            projects,
            follow_symlinks,
            screen: Screen::Projects,
        }
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn projects(&self) -> &[Project] {
        &self.projects
    }

    pub fn show_projects(&mut self) {
        self.screen = Screen::Projects;
    }

    pub fn select_project(
        &self,
        project_id: &str,
        openers: &Openers,
    ) -> Result<NavigationOutcome, NavigationError> {
        let project = self.project(project_id)?.clone();
        Ok(NavigationOutcome::Selected(Selection::Project {
            opener: resolve_opener(openers.project.as_deref(), &project, &project.path),
            project,
        }))
    }

    pub fn browse_project(
        &mut self,
        project_id: &str,
    ) -> Result<NavigationOutcome, NavigationError> {
        let project = self.project(project_id)?.clone();
        self.screen = Screen::Directory {
            project_id: project.id.clone(),
            path: project.path.clone(),
            ancestors: Vec::new(),
        };
        Ok(NavigationOutcome::LoadDirectory {
            path: project.path.clone(),
            project,
        })
    }

    pub fn select_host_item(
        &self,
        project_id: &str,
        id: &str,
    ) -> Result<NavigationOutcome, NavigationError> {
        let project = self.project(project_id)?.clone();
        Ok(NavigationOutcome::Selected(Selection::HostItem {
            project,
            id: id.to_string(),
        }))
    }

    pub fn close_project(&self, project_id: &str) -> Result<NavigationOutcome, NavigationError> {
        let project = self.project(project_id)?.clone();
        Ok(NavigationOutcome::Selected(Selection::CloseProject {
            project,
        }))
    }

    pub fn select_entry(
        &mut self,
        entry: &DirectoryEntry,
        openers: &Openers,
    ) -> Result<NavigationOutcome, NavigationError> {
        let Screen::Directory {
            project_id,
            path,
            ancestors,
        } = &self.screen
        else {
            return Err(NavigationError::InvalidScreen);
        };
        let project = self.project(project_id)?.clone();
        if entry.kind.is_directory(self.follow_symlinks) {
            let mut next_ancestors = ancestors.clone();
            next_ancestors.push(path.clone());
            self.screen = Screen::Directory {
                project_id: project.id.clone(),
                path: entry.path.clone(),
                ancestors: next_ancestors,
            };
            return Ok(NavigationOutcome::LoadDirectory {
                project,
                path: entry.path.clone(),
            });
        }

        Ok(NavigationOutcome::Selected(Selection::File {
            opener: resolve_opener(openers.file.as_deref(), &project, &entry.path),
            project,
            path: entry.path.clone(),
        }))
    }

    pub fn back(&mut self) -> Result<NavigationOutcome, NavigationError> {
        match self.screen.clone() {
            Screen::Projects => Ok(NavigationOutcome::Cancelled),
            Screen::Directory {
                project_id,
                path: _,
                mut ancestors,
            } => {
                if let Some(parent) = ancestors.pop() {
                    let project = self.project(&project_id)?.clone();
                    self.screen = Screen::Directory {
                        project_id,
                        path: parent.clone(),
                        ancestors,
                    };
                    Ok(NavigationOutcome::LoadDirectory {
                        project,
                        path: parent,
                    })
                } else {
                    self.screen = Screen::Projects;
                    Ok(NavigationOutcome::Continue)
                }
            }
        }
    }

    fn project(&self, project_id: &str) -> Result<&Project, NavigationError> {
        self.projects
            .iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| NavigationError::UnknownProject(project_id.to_string()))
    }
}

fn resolve_opener(
    template: Option<&[String]>,
    project: &Project,
    path: &std::path::Path,
) -> Option<Vec<String>> {
    template.map(|args| {
        args.iter()
            .map(|argument| {
                argument
                    .replace("{path}", &path.to_string_lossy())
                    .replace("{project.path}", &project.path.to_string_lossy())
                    .replace("{project.id}", &project.id)
                    .replace("{project.name}", &project.name)
                    .replace("{project.group}", &project.group)
            })
            .collect()
    })
}
