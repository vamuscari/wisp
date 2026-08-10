use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use thiserror::Error;

use crate::{model::DirectoryEntry, path::comparison_key};

pub const CACHE_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedDirectory {
    pub path: PathBuf,
    pub scanned_at: u64,
    pub entries: Vec<DirectoryEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CacheDocument {
    version: u32,
    config_fingerprint: String,
    #[serde(default)]
    generation: u64,
    directories: BTreeMap<String, CachedDirectory>,
}

impl CacheDocument {
    fn empty(config_fingerprint: String) -> Self {
        Self {
            version: CACHE_VERSION,
            config_fingerprint,
            generation: 0,
            directories: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct CacheStore {
    path: PathBuf,
    document: CacheDocument,
    base_generation: u64,
    dirty_directories: BTreeSet<String>,
    cleared: bool,
    replace_document: bool,
    stale: bool,
}

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("cache I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("cache serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
}

impl CacheStore {
    pub fn open(
        path: impl Into<PathBuf>,
        config_fingerprint: impl Into<String>,
    ) -> Result<Self, CacheError> {
        let path = path.into();
        let config_fingerprint = config_fingerprint.into();
        let (document, base_generation, replace_document) = match fs::read(&path) {
            Ok(contents) => match serde_json::from_slice::<CacheDocument>(&contents) {
                Ok(document)
                    if document.version == CACHE_VERSION
                        && document.config_fingerprint == config_fingerprint =>
                {
                    let generation = document.generation;
                    (document, generation, false)
                }
                Ok(document) => {
                    let generation = document.generation;
                    let mut empty = CacheDocument::empty(config_fingerprint.clone());
                    empty.generation = generation;
                    (empty, generation, true)
                }
                Err(_) => (CacheDocument::empty(config_fingerprint.clone()), 0, true),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                (CacheDocument::empty(config_fingerprint), 0, false)
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            document,
            base_generation,
            dirty_directories: BTreeSet::new(),
            cleared: false,
            replace_document,
            stale: false,
        })
    }

    pub fn directory(&self, path: &Path, now: u64, ttl_seconds: u64) -> Option<&CachedDirectory> {
        let key = comparison_key(&path.to_string_lossy());
        self.document.directories.get(&key).filter(|directory| {
            ttl_seconds > 0
                && now >= directory.scanned_at
                && now - directory.scanned_at < ttl_seconds
        })
    }

    pub fn put_directory(&mut self, path: &Path, entries: Vec<DirectoryEntry>, scanned_at: u64) {
        let key = comparison_key(&path.to_string_lossy());
        self.document.directories.insert(
            key.clone(),
            CachedDirectory {
                path: path.to_path_buf(),
                scanned_at,
                entries,
            },
        );
        self.dirty_directories.insert(key);
    }

    pub fn clear(&mut self) {
        self.document.directories.clear();
        self.dirty_directories.clear();
        self.cleared = true;
        self.stale = false;
    }

    pub fn save(&mut self) -> Result<(), CacheError> {
        let parent = self
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        fs::create_dir_all(&parent)?;

        let lock_path = lock_path(&self.path);
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(lock_path)?;
        lock.lock_exclusive()?;
        let result = self.save_locked(&parent);
        FileExt::unlock(&lock)?;
        result
    }

    fn save_locked(&mut self, parent: &Path) -> Result<(), CacheError> {
        let on_disk = self.read_current()?;
        let current_generation = match &on_disk {
            OnDisk::Document(document) => document.generation,
            OnDisk::Missing | OnDisk::Invalid => 0,
        };

        if self.cleared {
            self.document.generation = current_generation.saturating_add(1);
            self.write_atomic(parent)?;
            self.finish_save();
            return Ok(());
        }
        if self.stale {
            return Ok(());
        }

        match on_disk {
            OnDisk::Document(mut current)
                if current.version == CACHE_VERSION
                    && current.config_fingerprint == self.document.config_fingerprint =>
            {
                if current.generation != self.base_generation && !self.replace_document {
                    self.document = current;
                    self.finish_save();
                    return Ok(());
                }
                for key in &self.dirty_directories {
                    if let Some(directory) = self.document.directories.get(key) {
                        current.directories.insert(key.clone(), directory.clone());
                    }
                }
                self.document = current;
            }
            OnDisk::Document(current) => {
                if !self.replace_document || current.generation != self.base_generation {
                    self.dirty_directories.clear();
                    self.stale = true;
                    return Ok(());
                }
                self.document.generation = current.generation.saturating_add(1);
            }
            OnDisk::Invalid => {
                self.document.generation = self.base_generation.saturating_add(1);
            }
            OnDisk::Missing => {}
        }

        self.write_atomic(parent)?;
        self.finish_save();
        Ok(())
    }

    fn read_current(&self) -> Result<OnDisk, CacheError> {
        match fs::read(&self.path) {
            Ok(contents) => Ok(match serde_json::from_slice(&contents) {
                Ok(document) => OnDisk::Document(document),
                Err(_) => OnDisk::Invalid,
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(OnDisk::Missing),
            Err(error) => Err(error.into()),
        }
    }

    fn finish_save(&mut self) {
        self.base_generation = self.document.generation;
        self.dirty_directories.clear();
        self.cleared = false;
        self.replace_document = false;
        self.stale = false;
    }

    fn write_atomic(&self, parent: &Path) -> Result<(), CacheError> {
        let mut temporary = NamedTempFile::new_in(parent)?;
        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            serde_json::to_writer_pretty(&mut writer, &self.document)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        sync_parent(parent);
        Ok(())
    }
}

enum OnDisk {
    Missing,
    Invalid,
    Document(CacheDocument),
}

fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".lock");
    PathBuf::from(name)
}

fn sync_parent(parent: &Path) {
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
}
