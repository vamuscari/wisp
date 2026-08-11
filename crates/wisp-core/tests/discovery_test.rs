use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

use wisp_core::{
    config::Config,
    discovery::{DiscoveryError, FileSystem, discover_projects},
    model::{DirectoryEntry, EntryKind},
    path::comparison_key,
};

#[derive(Default)]
struct FakeFileSystem {
    directories: HashMap<PathBuf, Vec<DirectoryEntry>>,
}

impl FakeFileSystem {
    fn directory(mut self, path: &str, entries: Vec<(&str, EntryKind)>) -> Self {
        self.directories.insert(
            PathBuf::from(path),
            entries
                .into_iter()
                .map(|(entry, kind)| DirectoryEntry::new(PathBuf::from(entry), kind))
                .collect(),
        );
        self
    }
}

impl FileSystem for FakeFileSystem {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirectoryEntry>> {
        self.directories
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing fixture"))
    }
}

#[test]
fn discovers_typed_directories_deduplicates_paths_and_sorts_projects() {
    let config = Config::parse(
        r#"
version = 3

[[roots]]
path = "~/Repos"

[[roots]]
path = "~/work"

[[projects]]
id = "artifacts"
path = "~/Artifacts"
group = "Home"
name = "Artifacts"
"#,
        Path::new("/Users/test"),
    )
    .unwrap();
    let fs = FakeFileSystem::default()
        .directory(
            "/Users/test/Repos",
            vec![
                ("/Users/test/Repos/zeta", EntryKind::Directory),
                ("/Users/test/Repos/notes.txt", EntryKind::File),
                ("/Users/test/Repos/alpha.project", EntryKind::Directory),
            ],
        )
        .directory(
            "/Users/test/work",
            vec![
                ("/Users/test/work/beta", EntryKind::Directory),
                ("/Users/test/Repos/zeta", EntryKind::Directory),
            ],
        );

    let projects = discover_projects(&config, &fs).expect("discovery should succeed");

    assert_eq!(projects.len(), 4);
    assert_eq!(projects[0].name, "alpha.project");
    assert_eq!(projects[0].group, "Repos");
    assert_eq!(
        projects[0].path,
        Path::new("/Users/test/Repos/alpha.project")
    );
    assert_eq!(projects[1].id, "artifacts");
    assert_eq!(projects[1].name, "Artifacts");
    assert_eq!(projects[2].name, "beta");
    assert_eq!(projects[3].name, "zeta");
}

#[test]
fn follows_directory_symlinks_only_when_configured() {
    let mut config = Config::parse(
        r#"
version = 3
[[roots]]
path = "~/Repos"
"#,
        Path::new("/Users/test"),
    )
    .unwrap();
    let fs = FakeFileSystem::default().directory(
        "/Users/test/Repos",
        vec![("/Users/test/Repos/linked", EntryKind::SymlinkDirectory)],
    );

    assert!(discover_projects(&config, &fs).unwrap().is_empty());

    config.follow_symlinks = true;
    assert_eq!(discover_projects(&config, &fs).unwrap()[0].name, "linked");
}

#[test]
fn rejects_duplicate_explicit_project_ids() {
    let config = Config::parse(
        r#"
version = 3

[[projects]]
id = "api"
path = "/one/api"

[[projects]]
id = "api"
path = "/two/api"
"#,
        Path::new("/home/test"),
    )
    .unwrap();

    let error = discover_projects(&config, &FakeFileSystem::default())
        .expect_err("duplicate IDs should fail");
    assert!(matches!(error, DiscoveryError::DuplicateProjectId { id, .. } if id == "api"));
}

#[test]
fn comparison_keys_normalize_windows_unc_and_posix_paths() {
    assert_eq!(
        comparison_key(r"C:\Repos\Api"),
        comparison_key("c:/repos/group/../api/")
    );
    assert_eq!(
        comparison_key(r"\\Server\Share\Project"),
        comparison_key("//server/share/folder/../project/")
    );
    assert_ne!(comparison_key("/Repos/API"), comparison_key("/Repos/api"));
    assert_eq!(comparison_key("/Repos/group/../api/"), "/Repos/api");
}
