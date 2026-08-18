use std::{fs, path::Path};

use tempfile::tempdir;
use wisp_core::{
    cache::{CACHE_VERSION, CacheStore},
    config::Config,
    model::{DirectoryEntry, EntryKind},
};

#[test]
fn persists_typed_directory_entries_and_expires_at_the_ttl_boundary() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("cache.json");
    let directory = Path::new("/Users/test/Repos/api");
    let entries = vec![
        DirectoryEntry::new(directory.join("src"), EntryKind::Directory),
        DirectoryEntry::new(directory.join("README.md"), EntryKind::File),
    ];

    let mut cache = CacheStore::open(&path, "config-a").unwrap();
    cache.put_directory(directory, entries.clone(), 100);
    cache.save().unwrap();

    let cache = CacheStore::open(&path, "config-a").unwrap();
    assert_eq!(
        cache.directory(directory, 159, 60).unwrap().entries,
        entries
    );
    assert!(cache.directory(directory, 160, 60).is_none());
}

#[test]
fn invalidates_cache_for_config_or_schema_changes() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("cache.json");
    let directory = Path::new("/repo");

    let mut cache = CacheStore::open(&path, "config-a").unwrap();
    cache.put_directory(directory, vec![], 100);
    cache.save().unwrap();

    let changed = CacheStore::open(&path, "config-b").unwrap();
    assert!(changed.directory(directory, 100, 60).is_none());

    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["version"] = 999.into();
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let mut changed = CacheStore::open(&path, "config-a").unwrap();
    assert!(changed.directory(directory, 100, 60).is_none());
    changed.save().unwrap();

    let rebuilt: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(rebuilt["version"], CACHE_VERSION);
    assert!(rebuilt["directories"].as_object().unwrap().is_empty());
}

#[test]
fn malformed_cache_is_ignored_and_replaced_atomically() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("nested/cache.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"not json").unwrap();

    let mut cache = CacheStore::open(&path, "config-a").unwrap();
    assert!(cache.directory(Path::new("/repo"), 100, 60).is_none());
    cache.put_directory(Path::new("/repo"), vec![], 100);
    cache.save().unwrap();
    cache.put_directory(Path::new("/repo"), vec![], 110);
    cache.save().unwrap();

    let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["config_fingerprint"], "config-a");
    assert_eq!(value["directories"]["/repo"]["scanned_at"], 110);
}

#[test]
fn current_version_cache_missing_required_fields_is_rebuilt() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("cache.json");
    let directory = Path::new("/repo");
    let mut cache = CacheStore::open(&path, "config-a").unwrap();
    cache.put_directory(directory, vec![], 100);
    cache.save().unwrap();

    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document.as_object_mut().unwrap().remove("generation");
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();

    let mut rebuilt = CacheStore::open(&path, "config-a").unwrap();
    assert!(rebuilt.directory(directory, 100, 60).is_none());
    rebuilt.save().unwrap();

    let persisted: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(persisted.get("generation").is_some());
    assert!(persisted["directories"].as_object().unwrap().is_empty());
}

#[test]
fn current_version_cache_with_unknown_fields_is_rebuilt() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("cache.json");
    let directory = Path::new("/repo");
    let mut cache = CacheStore::open(&path, "config-a").unwrap();
    cache.put_directory(directory, vec![], 100);
    cache.save().unwrap();

    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["future_field"] = true.into();
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();

    let rebuilt = CacheStore::open(&path, "config-a").unwrap();
    assert!(rebuilt.directory(directory, 100, 60).is_none());
}

#[test]
fn current_version_cache_with_unknown_directory_fields_is_rebuilt() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("cache.json");
    let directory = Path::new("/repo");
    let mut cache = CacheStore::open(&path, "config-a").unwrap();
    cache.put_directory(directory, vec![], 100);
    cache.save().unwrap();

    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["directories"]["/repo"]["future_field"] = true.into();
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();

    let rebuilt = CacheStore::open(&path, "config-a").unwrap();
    assert!(rebuilt.directory(directory, 100, 60).is_none());
}

#[test]
fn current_version_cache_with_unknown_entry_fields_is_rebuilt() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("cache.json");
    let directory = Path::new("/repo");
    let mut cache = CacheStore::open(&path, "config-a").unwrap();
    cache.put_directory(
        directory,
        vec![DirectoryEntry::new(
            directory.join("src"),
            EntryKind::Directory,
        )],
        100,
    );
    cache.save().unwrap();

    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["directories"]["/repo"]["entries"][0]["future_field"] = true.into();
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();

    let rebuilt = CacheStore::open(&path, "config-a").unwrap();
    assert!(rebuilt.directory(directory, 100, 60).is_none());
}

#[test]
fn current_version_cache_with_duplicate_directory_keys_is_rebuilt() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("cache.json");
    let document = format!(
        r#"{{"version":{CACHE_VERSION},"config_fingerprint":"config-a","generation":0,"directories":{{"/repo":{{"path":"/repo","scanned_at":100,"entries":[]}},"/repo":{{"path":"/repo","scanned_at":101,"entries":[]}}}}}}"#
    );
    fs::write(&path, document).unwrap();

    let rebuilt = CacheStore::open(&path, "config-a").unwrap();
    assert!(rebuilt.directory(Path::new("/repo"), 101, 60).is_none());
}

#[test]
fn config_fingerprint_is_stable_and_changes_with_discovery_inputs() {
    let first = Config::parse(
        "version = 4\n[[roots]]\npath = '/one'\n",
        Path::new("/home/test"),
    )
    .unwrap();
    let same = Config::parse(
        "version = 4\n[[roots]]\npath = '/one'\n",
        Path::new("/different/home"),
    )
    .unwrap();
    let changed = Config::parse(
        "version = 4\n[[roots]]\npath = '/two'\n",
        Path::new("/home/test"),
    )
    .unwrap();

    assert_eq!(first.fingerprint(), same.fingerprint());
    assert_ne!(first.fingerprint(), changed.fingerprint());
}

#[test]
fn concurrent_writers_merge_directory_records_under_the_file_lock() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("cache.json");
    let mut first = CacheStore::open(&path, "config-a").unwrap();
    let mut second = CacheStore::open(&path, "config-a").unwrap();

    first.put_directory(Path::new("/one"), vec![], 100);
    second.put_directory(Path::new("/two"), vec![], 100);
    first.save().unwrap();
    second.save().unwrap();

    let merged = CacheStore::open(path, "config-a").unwrap();
    assert!(merged.directory(Path::new("/one"), 100, 60).is_some());
    assert!(merged.directory(Path::new("/two"), 100, 60).is_some());
}

#[test]
fn a_writer_opened_before_clear_cannot_restore_stale_records() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("cache.json");
    let mut seed = CacheStore::open(&path, "config-a").unwrap();
    seed.put_directory(Path::new("/seed"), vec![], 100);
    seed.save().unwrap();

    let mut stale = CacheStore::open(&path, "config-a").unwrap();
    stale.put_directory(Path::new("/stale"), vec![], 101);
    let mut clearing = CacheStore::open(&path, "config-a").unwrap();
    clearing.clear();
    clearing.save().unwrap();
    stale.save().unwrap();

    let cleared = CacheStore::open(path, "config-a").unwrap();
    assert!(cleared.directory(Path::new("/seed"), 102, 60).is_none());
    assert!(cleared.directory(Path::new("/stale"), 102, 60).is_none());
}

#[test]
fn a_writer_opened_before_a_version_change_cannot_replace_the_new_schema() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("cache.json");
    let mut seed = CacheStore::open(&path, "config-a").unwrap();
    seed.put_directory(Path::new("/seed"), vec![], 100);
    seed.save().unwrap();

    let mut stale = CacheStore::open(&path, "config-a").unwrap();
    stale.put_directory(Path::new("/stale"), vec![], 101);

    let future = br#"{"version":999,"future_schema":true}"#;
    fs::write(&path, future).unwrap();
    stale.save().unwrap();

    assert_eq!(fs::read(path).unwrap(), future);
}

#[test]
fn an_unsupported_cache_changed_after_open_is_not_replaced() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("cache.json");
    fs::write(&path, br#"{"version":999,"revision":1}"#).unwrap();

    let mut stale = CacheStore::open(&path, "config-a").unwrap();
    stale.put_directory(Path::new("/stale"), vec![], 101);

    let updated = br#"{"version":999,"revision":2}"#;
    fs::write(&path, updated).unwrap();
    stale.save().unwrap();

    assert_eq!(fs::read(path).unwrap(), updated);
}

#[test]
fn records_from_the_future_are_not_fresh_after_clock_rollback() {
    let temp = tempdir().unwrap();
    let mut cache = CacheStore::open(temp.path().join("cache.json"), "config-a").unwrap();
    cache.put_directory(Path::new("/future"), vec![], 200);

    assert!(cache.directory(Path::new("/future"), 100, 60).is_none());
}
