use std::{
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

use tempfile::TempDir;
use wisp_core::{
    model::Project,
    protocol::{OpenCodeStatusEnvelope, Selection, SelectionEnvelope, SelectionStatus},
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
version = 4
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
            .env_remove("WISP_CONFIG_FILE")
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
fn hidden_opencode_bridge_commands_register_and_unregister_an_instance() {
    let fixture = Fixture::new();
    let registry = fixture.home.join("registry");
    success(
        fixture
            .bare_command()
            .env("WISP_OPENCODE_REGISTRY_DIR", &registry)
            .args([
                "opencode",
                "register",
                "--server-url",
                "http://127.0.0.1:4096",
                "--directory",
                "/repos/wisp",
                "--project-path",
                "/repos/wisp",
                "--pid",
                "123",
                "--pane-id",
                "42",
                "--session-id",
                "ses_123",
                "--session-status",
                r#"{"type":"busy"}"#,
                "--waiting-permissions",
                "2",
                "--waiting-questions",
                "1",
                "--session-error",
                "provider failed",
            ])
            .output()
            .unwrap(),
    );
    let files = fs::read_dir(&registry)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    let registered: serde_json::Value =
        serde_json::from_slice(&fs::read(files[0].path()).unwrap()).unwrap();
    assert_eq!(registered["session_id"], "ses_123");
    assert_eq!(registered["session_activity"], "running");
    assert_eq!(registered["session_waiting"]["permissions"], 2);
    assert_eq!(registered["session_waiting"]["questions"], 1);
    assert_eq!(registered["session_error"], "provider failed");

    success(
        fixture
            .bare_command()
            .env("WISP_OPENCODE_REGISTRY_DIR", &registry)
            .args([
                "opencode",
                "unregister",
                "--directory",
                "/repos/wisp",
                "--pid",
                "123",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(fs::read_dir(registry).unwrap().count(), 0);
}

#[test]
fn opencode_status_emits_a_versioned_empty_summary_without_registrations() {
    let fixture = Fixture::new();
    let registry = fixture.home.join("registry");

    let output = success(
        fixture
            .bare_command()
            .env("WISP_OPENCODE_REGISTRY_DIR", &registry)
            .args(["opencode", "status", "--json"])
            .output()
            .unwrap(),
    );
    let status: OpenCodeStatusEnvelope = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(status.sessions.waiting, 0);
    assert_eq!(status.sessions.running, 0);
    assert_eq!(status.sessions.retrying, 0);
    assert_eq!(status.sessions.idle, 0);
    assert_eq!(status.sessions.error, 0);
}

#[test]
fn canonical_opencode_plugin_uses_in_process_state_and_argv_registrations() {
    let plugin = include_str!("../../../opencode/wisp.js");

    assert!(plugin.contains("spawnSync"));
    assert!(plugin.contains("shell: false"));
    let version_gate = plugin
        .find("health.version !== SUPPORTED_OPENCODE_VERSION")
        .expect("plugin should reject unsupported OpenCode versions");
    let healthy_check = plugin
        .find("health.healthy !== true")
        .expect("plugin should reject unhealthy OpenCode servers");
    let initial_registration = plugin
        .rfind("\n  register()\n")
        .expect("plugin should register after its version gate");
    assert!(plugin.contains("/global/health"));
    assert!(plugin.contains("client.session.status()"));
    assert!(plugin.contains("client.session.children({ path: { id: parentID } })"));
    assert!(plugin.contains("url: \"/permission\""));
    assert!(plugin.contains("url: \"/question\""));
    assert!(plugin.contains("ownsSession(event.properties?.sessionID)"));
    assert!(plugin.contains("revision === startingRevision"));
    assert!(!plugin.contains("fetch(new URL"));
    assert!(version_gate < healthy_check);
    assert!(version_gate < initial_registration);
    assert!(plugin.contains("session.created"));
    assert!(plugin.contains("session.error"));
    assert!(plugin.contains("session.status"));
    assert!(plugin.contains("permission.asked"));
    assert!(plugin.contains("permission.replied"));
    assert!(plugin.contains("question.asked"));
    assert!(plugin.contains("question.replied"));
    assert!(plugin.contains("question.rejected"));
    assert!(plugin.contains("--session-status"));
    assert!(plugin.contains("--waiting-permissions"));
    assert!(plugin.contains("--waiting-questions"));
    assert!(plugin.contains("--session-error"));
    assert!(plugin.contains("!sessionID && event.type === \"session.status\""));
    assert!(plugin.contains("setInterval(refreshAndRegister, 30_000)"));
    assert!(plugin.contains("clearInterval(heartbeat)"));
    assert!(plugin.contains("const projectPath = worktree === \"/\" ? directory : worktree"));
    assert!(plugin.contains("opencode\", \"register"));
    assert!(plugin.contains("opencode\", \"unregister"));
    assert!(!plugin.contains("exec("));
}

#[test]
fn pick_accepts_the_sessions_initial_view() {
    let fixture = Fixture::new();

    let output = success(
        fixture
            .bare_command()
            .args(["pick", "--help"])
            .output()
            .unwrap(),
    );

    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("projects, windows, sessions"));
    assert!(help.contains("--disable-sessions"));
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
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["protocol_version"], 4);
    let projects: Vec<Project> = serde_json::from_value(envelope["projects"].clone()).unwrap();

    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].name, "api");
    assert_eq!(projects[1].id, "artifacts");
}

#[test]
fn deploy_installs_one_versioned_bundle_and_stable_host_loaders() {
    let fixture = Fixture::new();
    let deployment_root = fixture.home.join("wisp-data");
    success(
        fixture
            .bare_command()
            .env("WISP_DEPLOY_ROOT", &deployment_root)
            .arg("deploy")
            .output()
            .unwrap(),
    );

    let active: serde_json::Value =
        serde_json::from_slice(&fs::read(deployment_root.join("active.json")).unwrap()).unwrap();
    assert_eq!(active["deployment_schema_version"], 4);
    let bundle_id = active["current_bundle_id"].as_str().unwrap();
    assert_eq!(bundle_id.len(), 64);
    assert!(active["previous_bundle_id"].is_null());

    let bundle = deployment_root.join("deployments").join(bundle_id);
    let executable = if cfg!(windows) {
        "bin/wisp.exe"
    } else {
        "bin/wisp"
    };
    assert!(bundle.join(executable).is_file());
    assert!(bundle.join("wezterm/init.lua").is_file());
    assert!(bundle.join("wezterm/options.lua").is_file());
    assert!(bundle.join("wezterm/client.lua").is_file());
    assert!(bundle.join("wezterm/workspace.lua").is_file());
    assert!(bundle.join("wezterm/picker.lua").is_file());
    assert!(bundle.join("wezterm/status.lua").is_file());
    assert!(bundle.join("nvim/lua/wisp/init.lua").is_file());
    assert!(bundle.join("nvim/doc/wisp.txt").is_file());
    assert!(bundle.join("opencode/wisp.js").is_file());

    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(bundle.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["deployment_schema_version"], 4);
    assert_eq!(manifest["bundle_id"], bundle_id);
    assert_eq!(manifest["package_version"], "0.5.0");
    assert_eq!(manifest["protocol_version"], 4);
    assert!(manifest["files"][executable].is_string());
    assert!(manifest["files"]["wezterm/status.lua"].is_string());
    assert!(manifest["files"]["opencode/wisp.js"].is_string());

    let config_home = fixture.config.parent().unwrap().parent().unwrap();
    let wezterm_loader = fs::read_to_string(config_home.join("wezterm/wisp/init.lua")).unwrap();
    assert!(wezterm_loader.contains("wezterm.run_child_process"));
    assert!(wezterm_loader.contains("wisp-deployment-v4"));
    let nvim_loader = fs::read_to_string(deployment_root.join("nvim/lua/wisp/init.lua")).unwrap();
    assert!(nvim_loader.contains("vim.system"));
}

#[test]
fn opencode_install_writes_a_stable_loader_for_the_active_bundle() {
    let fixture = Fixture::new();
    let deployment_root = fixture.home.join("wisp-data");
    let opencode_config = fixture.home.join("opencode-config");
    success(
        fixture
            .bare_command()
            .env("WISP_DEPLOY_ROOT", &deployment_root)
            .arg("deploy")
            .output()
            .unwrap(),
    );

    let output = success(
        fixture
            .bare_command()
            .env("WISP_DEPLOY_ROOT", &deployment_root)
            .env("WISP_OPENCODE_CONFIG_DIR", &opencode_config)
            .args(["opencode", "install"])
            .output()
            .unwrap(),
    );

    let loader = fs::read_to_string(opencode_config.join("plugins/wisp.js")).unwrap();
    assert!(loader.contains(&deployment_root.to_string_lossy().into_owned()));
    assert!(loader.contains("opencode/wisp.js"));
    assert!(loader.contains("shell: false"));
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("restart OpenCode")
    );
}

#[test]
fn deploy_verify_and_status_detect_bundle_corruption() {
    let fixture = Fixture::new();
    let deployment_root = fixture.home.join("wisp-data");
    let mut command = fixture.bare_command();
    command.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(command.arg("deploy").output().unwrap());

    let mut verify = fixture.bare_command();
    verify.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(verify.args(["deploy", "verify"]).output().unwrap());

    let mut status = fixture.bare_command();
    status.env("WISP_DEPLOY_ROOT", &deployment_root);
    let output = success(
        status
            .args(["deploy", "status", "--json"])
            .output()
            .unwrap(),
    );
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["deployment_schema_version"], 4);
    assert_eq!(status["valid"], true);
    assert!(status["bundle_id"].is_string());

    let active: serde_json::Value =
        serde_json::from_slice(&fs::read(deployment_root.join("active.json")).unwrap()).unwrap();
    let adapter = deployment_root
        .join("deployments")
        .join(active["current_bundle_id"].as_str().unwrap())
        .join("wezterm/init.lua");
    fs::write(adapter, "corrupt").unwrap();
    let manifest_path = deployment_root
        .join("deployments")
        .join(active["current_bundle_id"].as_str().unwrap())
        .join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["files"]["wezterm/init.lua"] = blake3::hash(b"corrupt").to_hex().to_string().into();
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let mut verify = fixture.bare_command();
    verify.env("WISP_DEPLOY_ROOT", &deployment_root);
    let output = verify.args(["deploy", "verify"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("bundle ID mismatch"));
}

#[test]
fn deploy_prune_keeps_the_current_bundle() {
    let fixture = Fixture::new();
    let deployment_root = fixture.home.join("wisp-data");
    let mut command = fixture.bare_command();
    command.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(command.arg("deploy").output().unwrap());

    let active: serde_json::Value =
        serde_json::from_slice(&fs::read(deployment_root.join("active.json")).unwrap()).unwrap();
    let current = active["current_bundle_id"].as_str().unwrap().to_string();
    let stale = "b".repeat(64);
    fs::create_dir_all(deployment_root.join("deployments").join(&stale)).unwrap();

    let mut prune = fixture.bare_command();
    prune.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(prune.args(["deploy", "prune"]).output().unwrap());

    assert!(deployment_root.join("deployments").join(current).is_dir());
    assert!(!deployment_root.join("deployments").join(stale).exists());
}

#[test]
fn redeploying_the_active_bundle_preserves_the_previous_bundle() {
    let fixture = Fixture::new();
    let deployment_root = fixture.home.join("wisp-data");
    let mut command = fixture.bare_command();
    command.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(command.arg("deploy").output().unwrap());

    let active_path = deployment_root.join("active.json");
    let mut active: serde_json::Value =
        serde_json::from_slice(&fs::read(&active_path).unwrap()).unwrap();
    let previous = "a".repeat(64);
    active["previous_bundle_id"] = previous.clone().into();
    fs::write(&active_path, serde_json::to_vec_pretty(&active).unwrap()).unwrap();

    let mut command = fixture.bare_command();
    command.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(command.arg("deploy").output().unwrap());

    let active: serde_json::Value =
        serde_json::from_slice(&fs::read(active_path).unwrap()).unwrap();
    assert_eq!(active["previous_bundle_id"], previous);
}

#[test]
fn deploy_prune_refuses_a_nonexistent_active_bundle() {
    let fixture = Fixture::new();
    let deployment_root = fixture.home.join("wisp-data");
    let mut command = fixture.bare_command();
    command.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(command.arg("deploy").output().unwrap());

    let active_path = deployment_root.join("active.json");
    let mut active: serde_json::Value =
        serde_json::from_slice(&fs::read(&active_path).unwrap()).unwrap();
    let recoverable = active["current_bundle_id"].as_str().unwrap().to_string();
    active["current_bundle_id"] = "c".repeat(64).into();
    fs::write(&active_path, serde_json::to_vec_pretty(&active).unwrap()).unwrap();

    let mut prune = fixture.bare_command();
    prune.env("WISP_DEPLOY_ROOT", &deployment_root);
    let output = prune.args(["deploy", "prune"]).output().unwrap();
    assert!(!output.status.success());
    assert!(
        deployment_root
            .join("deployments")
            .join(recoverable)
            .is_dir()
    );
}

#[test]
fn deploy_prune_refuses_a_nonexistent_previous_bundle() {
    let fixture = Fixture::new();
    let deployment_root = fixture.home.join("wisp-data");
    let mut command = fixture.bare_command();
    command.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(command.arg("deploy").output().unwrap());

    let active_path = deployment_root.join("active.json");
    let mut active: serde_json::Value =
        serde_json::from_slice(&fs::read(&active_path).unwrap()).unwrap();
    active["previous_bundle_id"] = "c".repeat(64).into();
    fs::write(&active_path, serde_json::to_vec_pretty(&active).unwrap()).unwrap();
    let recoverable = "b".repeat(64);
    fs::create_dir_all(deployment_root.join("deployments").join(&recoverable)).unwrap();

    let mut prune = fixture.bare_command();
    prune.env("WISP_DEPLOY_ROOT", &deployment_root);
    let output = prune.args(["deploy", "prune"]).output().unwrap();
    assert!(!output.status.success());
    assert!(
        deployment_root
            .join("deployments")
            .join(recoverable)
            .is_dir()
    );
}

#[test]
fn deploy_prune_keeps_a_valid_previous_release() {
    let fixture = Fixture::new();
    let deployment_root = fixture.home.join("wisp-data");
    let mut command = fixture.bare_command();
    command.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(command.arg("deploy").output().unwrap());

    let active_path = deployment_root.join("active.json");
    let mut active: serde_json::Value =
        serde_json::from_slice(&fs::read(&active_path).unwrap()).unwrap();
    let current_id = active["current_bundle_id"].as_str().unwrap();
    let current = deployment_root.join("deployments").join(current_id);
    let executable = if cfg!(windows) {
        "bin/wisp.exe"
    } else {
        "bin/wisp"
    };
    let paths = [
        executable,
        "wezterm/init.lua",
        "wezterm/options.lua",
        "wezterm/client.lua",
        "wezterm/workspace.lua",
        "wezterm/picker.lua",
        "wezterm/status.lua",
        "nvim/lua/wisp/init.lua",
        "nvim/doc/wisp.txt",
        "opencode/wisp.js",
    ];
    let mut assets = paths.map(|relative| (relative, fs::read(current.join(relative)).unwrap()));
    assets[3].1.extend_from_slice(b"\nprevious release\n");

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"wisp-deployment-v4\0");
    for (relative, contents) in &assets {
        hasher.update(relative.as_bytes());
        hasher.update(b"\0");
        hasher.update(&(contents.len() as u64).to_le_bytes());
        hasher.update(contents);
    }
    let previous_id = hasher.finalize().to_hex().to_string();
    let previous = deployment_root.join("deployments").join(&previous_id);
    for (relative, contents) in &assets {
        let destination = previous.join(relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, contents).unwrap();
    }
    fs::set_permissions(
        previous.join(executable),
        fs::metadata(current.join(executable))
            .unwrap()
            .permissions(),
    )
    .unwrap();

    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(current.join("manifest.json")).unwrap()).unwrap();
    manifest["bundle_id"] = previous_id.clone().into();
    manifest["package_version"] = "0.1.0".into();
    manifest["protocol_version"] = 1.into();
    for (relative, contents) in &assets {
        manifest["files"][relative] = blake3::hash(contents).to_hex().to_string().into();
    }
    fs::write(
        previous.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    active["previous_bundle_id"] = previous_id.clone().into();
    fs::write(&active_path, serde_json::to_vec_pretty(&active).unwrap()).unwrap();
    let stale = "b".repeat(64);
    fs::create_dir_all(deployment_root.join("deployments").join(&stale)).unwrap();

    let mut prune = fixture.bare_command();
    prune.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(prune.args(["deploy", "prune"]).output().unwrap());
    assert!(previous.is_dir());
    assert!(!deployment_root.join("deployments").join(stale).exists());
}

#[test]
fn deploy_verify_rejects_a_corrupted_stable_loader() {
    let fixture = Fixture::new();
    let deployment_root = fixture.home.join("wisp-data");
    let mut command = fixture.bare_command();
    command.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(command.arg("deploy").output().unwrap());

    let config_home = fixture.config.parent().unwrap().parent().unwrap();
    fs::write(config_home.join("wezterm/wisp/init.lua"), "corrupt").unwrap();

    let mut verify = fixture.bare_command();
    verify.env("WISP_DEPLOY_ROOT", &deployment_root);
    let output = verify.args(["deploy", "verify"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("stable host loader"));
}

#[cfg(unix)]
#[test]
fn deploy_verify_rejects_a_non_executable_binary() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let deployment_root = fixture.home.join("wisp-data");
    let mut command = fixture.bare_command();
    command.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(command.arg("deploy").output().unwrap());

    let active: serde_json::Value =
        serde_json::from_slice(&fs::read(deployment_root.join("active.json")).unwrap()).unwrap();
    let executable = deployment_root
        .join("deployments")
        .join(active["current_bundle_id"].as_str().unwrap())
        .join("bin/wisp");
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o644);
    fs::set_permissions(executable, permissions).unwrap();

    let mut verify = fixture.bare_command();
    verify.env("WISP_DEPLOY_ROOT", &deployment_root);
    let output = verify.args(["deploy", "verify"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not executable"));
}

#[test]
fn deployed_runtime_check_rejects_bundle_corruption() {
    let fixture = Fixture::new();
    let deployment_root = fixture.home.join("wisp-data");
    let mut command = fixture.bare_command();
    command.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(command.arg("deploy").output().unwrap());

    let active: serde_json::Value =
        serde_json::from_slice(&fs::read(deployment_root.join("active.json")).unwrap()).unwrap();
    let bundle_id = active["current_bundle_id"].as_str().unwrap();
    let bundle = deployment_root.join("deployments").join(bundle_id);
    let executable = bundle.join(if cfg!(windows) {
        "bin/wisp.exe"
    } else {
        "bin/wisp"
    });

    success(
        Command::new(&executable)
            .args(["deploy", "check-bundle"])
            .arg(&deployment_root)
            .arg(bundle_id)
            .output()
            .unwrap(),
    );

    fs::write(bundle.join("wezterm/status.lua"), "corrupt").unwrap();
    let output = Command::new(executable)
        .args(["deploy", "check-bundle"])
        .arg(&deployment_root)
        .arg(bundle_id)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("checksum mismatch"));
}

#[test]
fn concurrent_deploys_activate_one_complete_bundle() {
    let fixture = Fixture::new();
    let deployment_root = fixture.home.join("wisp-data");
    let mut children = Vec::new();
    for _ in 0..4 {
        let mut command = fixture.bare_command();
        children.push(
            command
                .env("WISP_DEPLOY_ROOT", &deployment_root)
                .arg("deploy")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    for child in children {
        success(child.wait_with_output().unwrap());
    }

    let deployments = fs::read_dir(deployment_root.join("deployments"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(deployments.len(), 1);
    assert!(deployments[0].file_type().unwrap().is_dir());

    let mut verify = fixture.bare_command();
    verify.env("WISP_DEPLOY_ROOT", &deployment_root);
    success(verify.args(["deploy", "verify"]).output().unwrap());
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
    assert_eq!(envelope.protocol_version, 4);
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
    assert!(stdout.contains("--active-project-path"));
    assert!(stdout.contains("--active-file"));
    assert!(stdout.contains("--initial-view"));
    assert!(!stdout.contains("--annotations-file"));
}

#[test]
fn pick_writes_a_v4_error_envelope_for_a_v1_host_context() {
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
    assert_eq!(envelope.protocol_version, 4);
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

#[cfg(unix)]
#[test]
fn open_executes_an_opencode_session_opener() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let script = fixture.home.join("fake-opencode");
    let marker = fixture.home.join("attached.txt");
    fs::write(
        &script,
        "#!/bin/sh\nprintf '%s %s %s %s %s %s' \"$1\" \"$2\" \"$3\" \"$4\" \"$5\" \"$6\" > \"$7\"\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let envelope = SelectionEnvelope::selected(Selection::OpenCodeSession {
        project: Project {
            id: "api".into(),
            path: fixture.home.join("Repos/api"),
            group: "Repos".into(),
            name: "api".into(),
            display_name: "api".into(),
        },
        session_id: "ses_123".into(),
        opener: vec![
            script.to_string_lossy().into_owned(),
            "attach".into(),
            "http://127.0.0.1:4096".into(),
            "--dir".into(),
            "/repos/api".into(),
            "--session".into(),
            "ses_123".into(),
            marker.to_string_lossy().into_owned(),
        ],
        host_item_id: None,
    });
    let json = serde_json::to_string(&envelope).unwrap();

    success(fixture.command().args(["open", &json]).output().unwrap());

    assert_eq!(
        fs::read_to_string(marker).unwrap(),
        "attach http://127.0.0.1:4096 --dir /repos/api --session ses_123"
    );
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

    let unsupported = r#"{"protocol_version":1,"status":"future","future_field":true}"#;
    let output = fixture
        .command()
        .args(["open", unsupported])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported selection protocol"));
}
