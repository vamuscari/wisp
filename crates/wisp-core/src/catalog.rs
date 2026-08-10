use std::{cell::RefCell, io, path::Path};

use thiserror::Error;

use crate::{
    cache::{CacheError, CacheStore},
    config::Config,
    discovery::{DiscoveryError, FileSystem, discover_projects},
    model::{DirectoryEntry, Project},
};

#[derive(Debug)]
pub struct Catalog<F> {
    config: Config,
    file_system: F,
    cache: CacheStore,
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error(transparent)]
    Cache(#[from] CacheError),
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
    #[error("filesystem operation failed: {0}")]
    Io(#[from] io::Error),
}

impl<F: FileSystem> Catalog<F> {
    pub fn new(config: Config, file_system: F, cache: CacheStore) -> Self {
        Self {
            config,
            file_system,
            cache,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn projects(&mut self, now: u64) -> Result<Vec<Project>, CatalogError> {
        let projects = {
            let cached = CachingFileSystem::new(
                &self.file_system,
                &mut self.cache,
                now,
                self.config.cache_ttl_seconds,
            );
            discover_projects(&self.config, &cached)?
        };
        self.cache.save()?;
        Ok(projects)
    }

    pub fn directory(
        &mut self,
        path: &Path,
        now: u64,
    ) -> Result<Vec<DirectoryEntry>, CatalogError> {
        let mut entries = if let Some(directory) =
            self.cache
                .directory(path, now, self.config.cache_ttl_seconds)
        {
            directory.entries.clone()
        } else {
            let entries = self.file_system.read_dir(path)?;
            self.cache.put_directory(path, entries.clone(), now);
            self.cache.save()?;
            entries
        };
        sort_entries(&mut entries);
        Ok(entries)
    }

    pub fn refresh_directory(
        &mut self,
        path: &Path,
        now: u64,
    ) -> Result<Vec<DirectoryEntry>, CatalogError> {
        let mut entries = self.file_system.read_dir(path)?;
        self.cache.put_directory(path, entries.clone(), now);
        self.cache.save()?;
        sort_entries(&mut entries);
        Ok(entries)
    }

    pub fn refresh(&mut self, now: u64) -> Result<Vec<Project>, CatalogError> {
        self.cache.clear();
        let projects = {
            let cached = CachingFileSystem::new(&self.file_system, &mut self.cache, now, 0);
            let projects = discover_projects(&self.config, &cached)?;
            for project in &projects {
                let _ = cached.read_dir(&project.path);
            }
            projects
        };
        self.cache.save()?;
        Ok(projects)
    }
}

struct CachingFileSystem<'a, F> {
    inner: &'a F,
    cache: RefCell<&'a mut CacheStore>,
    now: u64,
    ttl_seconds: u64,
}

impl<'a, F> CachingFileSystem<'a, F> {
    fn new(inner: &'a F, cache: &'a mut CacheStore, now: u64, ttl_seconds: u64) -> Self {
        Self {
            inner,
            cache: RefCell::new(cache),
            now,
            ttl_seconds,
        }
    }
}

impl<F: FileSystem> FileSystem for CachingFileSystem<'_, F> {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirectoryEntry>> {
        if let Some(entries) = self
            .cache
            .borrow()
            .directory(path, self.now, self.ttl_seconds)
            .map(|directory| directory.entries.clone())
        {
            return Ok(entries);
        }

        let entries = self.inner.read_dir(path)?;
        self.cache
            .borrow_mut()
            .put_directory(path, entries.clone(), self.now);
        Ok(entries)
    }
}

fn sort_entries(entries: &mut [DirectoryEntry]) {
    entries.sort_by(|left, right| {
        left.name()
            .to_lowercase()
            .cmp(&right.name().to_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
}
