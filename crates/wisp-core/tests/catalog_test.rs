use std::{
    cell::RefCell,
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    rc::Rc,
};

use tempfile::tempdir;
use wisp_core::{
    cache::CacheStore,
    catalog::Catalog,
    config::Config,
    discovery::FileSystem,
    model::{DirectoryEntry, EntryKind},
};

#[derive(Clone, Default)]
struct CountingFileSystem {
    directories: HashMap<PathBuf, Vec<DirectoryEntry>>,
    calls: Rc<RefCell<HashMap<PathBuf, usize>>>,
}

impl CountingFileSystem {
    fn directory(mut self, path: &str, entries: Vec<(&str, EntryKind)>) -> Self {
        self.directories.insert(
            PathBuf::from(path),
            entries
                .into_iter()
                .map(|(path, kind)| DirectoryEntry::new(PathBuf::from(path), kind))
                .collect(),
        );
        self
    }

    fn calls(&self, path: &str) -> usize {
        self.calls
            .borrow()
            .get(Path::new(path))
            .copied()
            .unwrap_or_default()
    }
}

impl FileSystem for CountingFileSystem {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirectoryEntry>> {
        *self
            .calls
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_default() += 1;
        self.directories
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing fixture"))
    }
}

fn config() -> Config {
    Config::parse(
        r#"
version = 3
cache_ttl_seconds = 60

[[roots]]
path = "~/Repos"

[[projects]]
id = "artifacts"
path = "~/Artifacts"
name = "Artifacts"
"#,
        Path::new("/Users/test"),
    )
    .unwrap()
}

#[test]
fn project_and_directory_reads_share_the_persistent_cache() {
    let temp = tempdir().unwrap();
    let file_system = CountingFileSystem::default()
        .directory(
            "/Users/test/Repos",
            vec![("/Users/test/Repos/api", EntryKind::Directory)],
        )
        .directory(
            "/Users/test/Repos/api",
            vec![("/Users/test/Repos/api/src", EntryKind::Directory)],
        )
        .directory("/Users/test/Artifacts", vec![]);
    let counts = file_system.clone();
    let cache = CacheStore::open(temp.path().join("cache.json"), config().fingerprint()).unwrap();
    let mut catalog = Catalog::new(config(), file_system, cache);

    assert_eq!(catalog.projects(100).unwrap().len(), 2);
    assert_eq!(catalog.projects(101).unwrap().len(), 2);
    assert_eq!(counts.calls("/Users/test/Repos"), 1);

    assert_eq!(
        catalog
            .directory(Path::new("/Users/test/Repos/api"), 100)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        catalog
            .directory(Path::new("/Users/test/Repos/api"), 159)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(counts.calls("/Users/test/Repos/api"), 1);

    catalog
        .directory(Path::new("/Users/test/Repos/api"), 160)
        .unwrap();
    assert_eq!(counts.calls("/Users/test/Repos/api"), 2);
}

#[test]
fn refresh_clears_cache_and_preloads_roots_and_project_directories_only() {
    let temp = tempdir().unwrap();
    let file_system = CountingFileSystem::default()
        .directory(
            "/Users/test/Repos",
            vec![("/Users/test/Repos/api", EntryKind::Directory)],
        )
        .directory(
            "/Users/test/Repos/api",
            vec![("/Users/test/Repos/api/src", EntryKind::Directory)],
        )
        .directory("/Users/test/Repos/api/src", vec![])
        .directory("/Users/test/Artifacts", vec![]);
    let counts = file_system.clone();
    let cache = CacheStore::open(temp.path().join("cache.json"), config().fingerprint()).unwrap();
    let mut catalog = Catalog::new(config(), file_system, cache);

    let projects = catalog.refresh(200).unwrap();

    assert_eq!(projects.len(), 2);
    assert_eq!(counts.calls("/Users/test/Repos"), 1);
    assert_eq!(counts.calls("/Users/test/Repos/api"), 1);
    assert_eq!(counts.calls("/Users/test/Artifacts"), 1);
    assert_eq!(counts.calls("/Users/test/Repos/api/src"), 0);
    catalog
        .directory(Path::new("/Users/test/Repos/api"), 201)
        .unwrap();
    assert_eq!(counts.calls("/Users/test/Repos/api"), 1);
}

#[test]
fn refreshing_one_directory_bypasses_its_unexpired_cache_record() {
    let temp = tempdir().unwrap();
    let file_system = CountingFileSystem::default()
        .directory("/Users/test/Repos", vec![])
        .directory(
            "/Users/test/Repos/api",
            vec![("/Users/test/Repos/api/src", EntryKind::Directory)],
        );
    let counts = file_system.clone();
    let cache = CacheStore::open(temp.path().join("cache.json"), config().fingerprint()).unwrap();
    let mut catalog = Catalog::new(config(), file_system, cache);

    catalog
        .directory(Path::new("/Users/test/Repos/api"), 100)
        .unwrap();
    catalog
        .refresh_directory(Path::new("/Users/test/Repos/api"), 101)
        .unwrap();

    assert_eq!(counts.calls("/Users/test/Repos/api"), 2);
    catalog
        .directory(Path::new("/Users/test/Repos/api"), 102)
        .unwrap();
    assert_eq!(counts.calls("/Users/test/Repos/api"), 2);
}
