use std::{collections::BTreeMap, fs};

use tempfile::tempdir;
use wisp_core::{
    discovery::{FileSystem, StdFileSystem},
    model::EntryKind,
};

#[test]
fn classifies_real_directories_and_files() {
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("README.md"), "read me").unwrap();

    let entries = StdFileSystem.read_dir(temp.path()).unwrap();
    let kinds = entries
        .into_iter()
        .map(|entry| (entry.name(), entry.kind))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(kinds["src"], EntryKind::Directory);
    assert_eq!(kinds["README.md"], EntryKind::File);
}

#[cfg(unix)]
#[test]
fn classifies_symlink_targets_without_following_them_during_iteration() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join("target-dir")).unwrap();
    fs::write(temp.path().join("target-file"), "content").unwrap();
    symlink("target-dir", temp.path().join("linked-dir")).unwrap();
    symlink("target-file", temp.path().join("linked-file")).unwrap();
    symlink("missing", temp.path().join("broken")).unwrap();

    let entries = StdFileSystem.read_dir(temp.path()).unwrap();
    let kinds = entries
        .into_iter()
        .map(|entry| (entry.name(), entry.kind))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(kinds["linked-dir"], EntryKind::SymlinkDirectory);
    assert_eq!(kinds["linked-file"], EntryKind::SymlinkFile);
    assert_eq!(kinds["broken"], EntryKind::Symlink);
}

#[cfg(target_os = "linux")]
#[test]
fn omits_names_that_cannot_be_represented_in_json_protocols() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join("valid")).unwrap();
    fs::create_dir(temp.path().join(OsString::from_vec(vec![b'i', 0xff]))).unwrap();

    let entries = StdFileSystem.read_dir(temp.path()).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name(), "valid");
}
