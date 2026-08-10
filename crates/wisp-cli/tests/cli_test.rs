use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

use tempfile::TempDir;
use wisp_core::{
    model::Project,
    protocol::{Selection, SelectionEnvelope, SelectionStatus},
};

struct Fixture {
    _temp: TempDir,
    home: PathBuf,
    config: PathBuf,
    cache_home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let config_dir = temp.path().join("config").join("wisp");
        let cache_home = temp.path().join("cache");
        fs::create_dir_all(home.join("Repos/api")).unwrap();
        fs::create_dir_all(home.join("Artifacts")).unwrap();
        fs::create_dir_all(&config_dir).unwrap();
        let config = config_dir.join("config.toml");
        fs::write(
            &config,
            r#"
version = 1
cache_ttl_seconds = 60

[[roots]]
path = "~/Repos"
group = "Repos"

[[projects]]
id = "artifacts"
path = "~/Artifacts"
group = "Home"
name = "Artifacts"
"#,
        )
        .unwrap();
        Self {
            _temp: temp,
            home,
            config,
            cache_home,
        }
    }

    fn command(&self) -> Command {
        let mut command = self.bare_command();
        command.arg("--config").arg(&self.config);
        command
    }

    fn bare_command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wisp"));
        command
            .env("HOME", &self.home)
            .env(
                "XDG_CONFIG_HOME",
                self.config.parent().unwrap().parent().unwrap(),
            )
            .env("XDG_CACHE_HOME", &self.cache_home);
        command
    }
}

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "command failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn validates_configuration_and_lists_discovered_projects_as_json() {
    let fixture = Fixture::new();
    let output = success(
        fixture
            .command()
            .args(["config", "validate"])
            .output()
            .unwrap(),
    );
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("configuration is valid")
    );

    let output = success(
        fixture
            .command()
            .args(["projects", "--json"])
            .output()
            .unwrap(),
    );
    let projects: Vec<Project> = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].name, "api");
    assert_eq!(projects[1].id, "artifacts");
}

#[test]
fn refresh_populates_the_disk_cache_and_cache_clear_empties_it() {
    let fixture = Fixture::new();
    let output = success(fixture.command().arg("refresh").output().unwrap());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("refreshed 2 projects")
    );

    let cache_path = fixture.cache_home.join("wisp/cache.json");
    let populated: serde_json::Value =
        serde_json::from_slice(&fs::read(&cache_path).unwrap()).unwrap();
    assert!(!populated["directories"].as_object().unwrap().is_empty());

    success(fixture.command().args(["cache", "clear"]).output().unwrap());
    let cleared: serde_json::Value =
        serde_json::from_slice(&fs::read(cache_path).unwrap()).unwrap();
    assert!(cleared["directories"].as_object().unwrap().is_empty());
}

#[test]
fn pick_writes_a_versioned_error_atomically_when_setup_fails() {
    let fixture = Fixture::new();
    let missing = fixture.home.join("missing.toml");
    let result_path = fixture.home.join("result.json");
    let output = fixture
        .bare_command()
        .args([
            "--config",
            missing.to_str().unwrap(),
            "pick",
            "--result-file",
        ])
        .arg(&result_path)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let envelope: SelectionEnvelope =
        serde_json::from_slice(&fs::read(&result_path).unwrap()).unwrap();
    assert_eq!(envelope.protocol_version, 2);
    assert_eq!(envelope.status, SelectionStatus::Error);
    assert!(envelope.error.unwrap().contains("missing.toml"));
    assert_eq!(
        fs::read_dir(result_path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
            .count(),
        0
    );
}

#[test]
fn pick_help_exposes_host_context_and_initial_view_options() {
    let fixture = Fixture::new();
    let output = success(fixture.command().args(["pick", "--help"]).output().unwrap());
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(stdout.contains("--host-context-file"));
    assert!(stdout.contains("--initial-view"));
    assert!(!stdout.contains("--annotations-file"));
}

#[test]
fn pick_writes_a_v2_error_envelope_for_a_v1_host_context() {
    let fixture = Fixture::new();
    let context_path = fixture.home.join("context.json");
    let result_path = fixture.home.join("result.json");
    fs::write(&context_path, r#"{"protocol_version":1,"projects":{}}"#).unwrap();

    let output = fixture
        .command()
        .args(["pick", "--result-file"])
        .arg(&result_path)
        .args(["--host-context-file"])
        .arg(&context_path)
        .args(["--initial-view", "windows"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let envelope: SelectionEnvelope =
        serde_json::from_slice(&fs::read(&result_path).unwrap()).unwrap();
    assert_eq!(envelope.protocol_version, 2);
    assert_eq!(envelope.status, SelectionStatus::Error);
    assert!(
        envelope
            .error
            .unwrap()
            .contains("unsupported host context version 1")
    );
}

#[test]
fn no_subcommand_defaults_to_pick_and_uses_the_xdg_config_path() {
    let fixture = Fixture::new();
    let empty_config_home = fixture.home.join("empty-config");
    fs::create_dir(&empty_config_home).unwrap();
    let output = fixture
        .bare_command()
        .env("XDG_CONFIG_HOME", &empty_config_home)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let envelope: SelectionEnvelope = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope.status, SelectionStatus::Error);
    assert!(
        envelope.error.unwrap().contains(
            &empty_config_home
                .join("wisp/config.toml")
                .to_string_lossy()
                .into_owned()
        )
    );
}

#[cfg(unix)]
#[test]
fn open_executes_the_resolved_argv_without_a_shell() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let script = fixture.home.join("opener with spaces");
    let marker = fixture.home.join("opened.txt");
    let injected = fixture.home.join("must-not-exist");
    fs::write(&script, "#!/bin/sh\nprintf '%s' \"$1\" > \"$2\"\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let argument = format!("literal; touch {}", injected.display());
    let envelope = SelectionEnvelope::selected(Selection::Project {
        project: Project {
            id: "api".into(),
            path: fixture.home.join("Repos/api"),
            group: "Repos".into(),
            name: "api".into(),
            display_name: "api".into(),
        },
        opener: Some(vec![
            script.to_string_lossy().into_owned(),
            argument.clone(),
            marker.to_string_lossy().into_owned(),
        ]),
    });
    let json = serde_json::to_string(&envelope).unwrap();

    success(fixture.command().args(["open", &json]).output().unwrap());

    assert_eq!(fs::read_to_string(marker).unwrap(), argument);
    assert!(!injected.exists());
}

#[test]
fn open_rejects_cancelled_results_and_missing_openers() {
    let fixture = Fixture::new();
    let cancelled = serde_json::to_string(&SelectionEnvelope::cancelled()).unwrap();
    let output = fixture
        .command()
        .args(["open", &cancelled])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("selected result"));

    let envelope = SelectionEnvelope::selected(Selection::Project {
        project: Project {
            id: "api".into(),
            path: PathBuf::from("/repos/api"),
            group: "Repos".into(),
            name: "api".into(),
            display_name: "api".into(),
        },
        opener: None,
    });
    let json = serde_json::to_string(&envelope).unwrap();
    let output = fixture.command().args(["open", &json]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no opener"));

    let unsupported = json.replace("\"protocol_version\":2", "\"protocol_version\":1");
    let output = fixture
        .command()
        .args(["open", &unsupported])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported selection protocol"));
}
