use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsStr,
    fs,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

use directories::BaseDirs;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use tempfile::{Builder, NamedTempFile};
use thiserror::Error;
use wisp_core::protocol::PROTOCOL_VERSION;

const DEPLOYMENT_SCHEMA_VERSION: u32 = PROTOCOL_VERSION;
const WEZTERM_ADAPTER: &[u8] = include_bytes!("../../../wezterm/init.lua");
const WEZTERM_OPTIONS: &[u8] = include_bytes!("../../../wezterm/options.lua");
const WEZTERM_CLIENT: &[u8] = include_bytes!("../../../wezterm/client.lua");
const WEZTERM_WORKSPACE: &[u8] = include_bytes!("../../../wezterm/workspace.lua");
const WEZTERM_PICKER: &[u8] = include_bytes!("../../../wezterm/picker.lua");
const WEZTERM_STATUS: &[u8] = include_bytes!("../../../wezterm/status.lua");
const NVIM_ADAPTER: &[u8] = include_bytes!("../../../nvim/lua/wisp/init.lua");
const NVIM_HELP: &[u8] = include_bytes!("../../../nvim/doc/wisp.txt");
const OPENCODE_PLUGIN: &[u8] = include_bytes!("../../../opencode/wisp.js");
const PRUNE_TOMBSTONE_PREFIX: &str = ".prune-";

#[derive(Debug, Error)]
pub enum DeployError {
    #[error("could not determine the platform data directory")]
    MissingDataDirectory,
    #[error("could not determine the WezTerm configuration directory")]
    MissingConfigDirectory,
    #[error("deployment I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("deployment JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error(
        "unsupported active deployment version in {path}; rerun wisp deploy --replace-incompatible to discard incompatible deployment state"
    )]
    IncompatibleActive { path: PathBuf },
    #[error("invalid deployment: {0}")]
    Invalid(String),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ActiveDeployment {
    deployment_schema_version: u32,
    current_bundle_id: String,
    previous_bundle_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    deployment_schema_version: u32,
    bundle_id: String,
    package_version: String,
    protocol_version: u32,
    files: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct DeploymentStatus {
    deployment_schema_version: u32,
    valid: bool,
    bundle_id: String,
    previous_bundle_id: Option<String>,
    deployment_root: PathBuf,
    wezterm_loader: PathBuf,
    nvim_runtime: PathBuf,
}

struct DeploymentLock(fs::File);

impl DeploymentLock {
    fn acquire(root: &Path) -> Result<Self, DeployError> {
        Self::open(root, true)
    }

    fn acquire_shared(root: &Path) -> Result<Self, DeployError> {
        Self::open(root, false)
    }

    fn open(root: &Path, exclusive: bool) -> Result<Self, DeployError> {
        fs::create_dir_all(root)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(root.join(".deploy.lock"))?;
        if exclusive {
            FileExt::lock_exclusive(&file)?;
        } else {
            FileExt::lock_shared(&file)?;
        }
        Ok(Self(file))
    }
}

impl Drop for DeploymentLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

pub fn deploy(replace_incompatible: bool) -> Result<PathBuf, DeployError> {
    let root = deployment_root()?;
    let _lock = DeploymentLock::acquire(&root)?;
    let wezterm_config = wezterm_config_dir()?;
    let executable = env::current_exe()?;
    let executable_name = if cfg!(windows) { "wisp.exe" } else { "wisp" };
    let executable_path = format!("bin/{executable_name}");
    let executable_bytes = fs::read(&executable)?;
    let assets = [
        (executable_path.as_str(), executable_bytes.as_slice()),
        ("wezterm/init.lua", WEZTERM_ADAPTER),
        ("wezterm/options.lua", WEZTERM_OPTIONS),
        ("wezterm/client.lua", WEZTERM_CLIENT),
        ("wezterm/workspace.lua", WEZTERM_WORKSPACE),
        ("wezterm/picker.lua", WEZTERM_PICKER),
        ("wezterm/status.lua", WEZTERM_STATUS),
        ("nvim/lua/wisp/init.lua", NVIM_ADAPTER),
        ("nvim/doc/wisp.txt", NVIM_HELP),
        ("opencode/wisp.js", OPENCODE_PLUGIN),
    ];
    let bundle_id = bundle_id(&assets);
    let deployments = root.join("deployments");
    fs::create_dir_all(&deployments)?;
    let destination = deployments.join(&bundle_id);

    if destination.exists() {
        verify_compatible_bundle(&destination, &bundle_id)?;
    } else {
        let staging = Builder::new().prefix(".wisp-").tempdir_in(&deployments)?;
        let mut files = BTreeMap::new();
        for (relative, contents) in assets {
            let path = staging.path().join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, contents)?;
            files.insert(
                relative.to_string(),
                blake3::hash(contents).to_hex().to_string(),
            );
        }
        make_executable(&staging.path().join(&executable_path))?;
        let manifest = Manifest {
            deployment_schema_version: DEPLOYMENT_SCHEMA_VERSION,
            bundle_id: bundle_id.clone(),
            package_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION,
            files,
        };
        write_json(staging.path().join("manifest.json"), &manifest)?;
        verify_compatible_bundle(staging.path(), &bundle_id)?;
        let staged = staging.keep();
        fs::rename(staged, &destination)?;
    }

    let active = match read_active(&root) {
        Ok(active) => active,
        Err(DeployError::IncompatibleActive { .. }) if replace_incompatible => None,
        Err(error) => return Err(error),
    };
    let previous = active.and_then(|active| {
        if active.current_bundle_id == bundle_id {
            active.previous_bundle_id
        } else {
            Some(active.current_bundle_id)
        }
    });
    write_stable_loaders(&root, &wezterm_config, executable_name)?;
    let active = ActiveDeployment {
        deployment_schema_version: DEPLOYMENT_SCHEMA_VERSION,
        current_bundle_id: bundle_id.clone(),
        previous_bundle_id: previous,
    };
    write_json_atomic(root.join("active.json"), &active)?;
    println!("deployed Wisp bundle {bundle_id}");
    Ok(destination)
}

pub fn verify() -> Result<(), DeployError> {
    let root = deployment_root()?;
    let _lock = DeploymentLock::acquire(&root)?;
    let active = read_active(&root)?
        .ok_or_else(|| DeployError::Invalid("no active Wisp deployment".into()))?;
    verify_compatible_bundle(
        &root.join("deployments").join(&active.current_bundle_id),
        &active.current_bundle_id,
    )?;
    verify_stable_loaders(&root, &wezterm_config_dir()?, executable_name())?;
    println!("verified Wisp bundle {}", active.current_bundle_id);
    Ok(())
}

pub fn status(json: bool) -> Result<(), DeployError> {
    let root = deployment_root()?;
    let _lock = DeploymentLock::acquire(&root)?;
    let active = read_active(&root)?
        .ok_or_else(|| DeployError::Invalid("no active Wisp deployment".into()))?;
    verify_compatible_bundle(
        &root.join("deployments").join(&active.current_bundle_id),
        &active.current_bundle_id,
    )?;
    verify_stable_loaders(&root, &wezterm_config_dir()?, executable_name())?;
    let status = DeploymentStatus {
        deployment_schema_version: DEPLOYMENT_SCHEMA_VERSION,
        valid: true,
        bundle_id: active.current_bundle_id,
        previous_bundle_id: active.previous_bundle_id,
        deployment_root: root.clone(),
        wezterm_loader: wezterm_config_dir()?.join("wisp/init.lua"),
        nvim_runtime: root.join("nvim"),
    };
    if json {
        let stdout = io::stdout();
        let mut writer = stdout.lock();
        serde_json::to_writer_pretty(&mut writer, &status)?;
        writer.write_all(b"\n")?;
    } else {
        println!("active Wisp bundle: {}", status.bundle_id);
        println!("deployment root: {}", status.deployment_root.display());
        println!("WezTerm loader: {}", status.wezterm_loader.display());
        println!("Neovim runtime: {}", status.nvim_runtime.display());
    }
    Ok(())
}

pub fn prune() -> Result<(), DeployError> {
    let root = deployment_root()?;
    let _lock = DeploymentLock::acquire(&root)?;
    let active = read_active(&root)?
        .ok_or_else(|| DeployError::Invalid("no active Wisp deployment".into()))?;
    verify_compatible_bundle(
        &root.join("deployments").join(&active.current_bundle_id),
        &active.current_bundle_id,
    )?;
    let mut keep = BTreeSet::from([active.current_bundle_id]);
    if let Some(previous) = active.previous_bundle_id {
        verify_bundle(&root.join("deployments").join(&previous), &previous)?;
        keep.insert(previous);
    }
    let deployments = root.join("deployments");
    let mut removed = 0;
    let mut retained = 0;
    let mut entries = fs::read_dir(&deployments)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        (!name.starts_with(PRUNE_TOMBSTONE_PREFIX), name)
    });
    for entry in entries {
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !keep.contains(&name) {
            match prune_deployment_directory(&deployments, &entry.path(), &name)? {
                PruneOutcome::Removed => removed += 1,
                PruneOutcome::Retained => retained += 1,
            }
        }
    }
    if retained == 0 {
        println!("pruned {removed} Wisp deployment(s)");
    } else {
        println!("pruned {removed} Wisp deployment(s); retained {retained} in-use deployment(s)");
    }
    Ok(())
}

enum PruneOutcome {
    Removed,
    Retained,
}

fn prune_deployment_directory(
    deployments: &Path,
    path: &Path,
    name: &str,
) -> Result<PruneOutcome, DeployError> {
    let tombstone = if name.starts_with(PRUNE_TOMBSTONE_PREFIX) {
        path.to_path_buf()
    } else {
        let tombstone = deployments.join(format!("{PRUNE_TOMBSTONE_PREFIX}{name}"));
        if tombstone.exists() {
            return Ok(PruneOutcome::Retained);
        }
        match fs::rename(path, &tombstone) {
            Ok(()) => tombstone,
            Err(error) if is_in_use_error(&error) => return Ok(PruneOutcome::Retained),
            Err(error) => return Err(error.into()),
        }
    };

    match fs::remove_dir_all(tombstone) {
        Ok(()) => Ok(PruneOutcome::Removed),
        Err(error) if is_in_use_error(&error) => Ok(PruneOutcome::Retained),
        Err(error) => Err(error.into()),
    }
}

#[cfg(windows)]
fn is_in_use_error(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32 | 33))
}

#[cfg(not(windows))]
fn is_in_use_error(_error: &io::Error) -> bool {
    false
}

pub fn check_bundle(root: &Path, bundle_id: &str) -> Result<(), DeployError> {
    validate_bundle_id(bundle_id)?;
    let _lock = DeploymentLock::acquire_shared(root)?;
    verify_compatible_bundle(&root.join("deployments").join(bundle_id), bundle_id)?;
    Ok(())
}

pub fn install_opencode() -> Result<PathBuf, DeployError> {
    let root = deployment_root()?;
    let _lock = DeploymentLock::acquire_shared(&root)?;
    let active = read_active(&root)?.ok_or_else(|| {
        DeployError::Invalid("no active Wisp deployment; run wisp deploy first".into())
    })?;
    verify_compatible_bundle(
        &root.join("deployments").join(&active.current_bundle_id),
        &active.current_bundle_id,
    )?;
    let path = opencode_config_dir()?.join("plugins/wisp.js");
    let loader = opencode_loader_contents(&root, executable_name())?;
    write_atomic(path.clone(), loader.as_bytes())?;
    println!(
        "installed OpenCode integration at {}; restart OpenCode to load it",
        path.display()
    );
    Ok(path)
}

fn deployment_root() -> Result<PathBuf, DeployError> {
    if let Some(path) = env::var_os("WISP_DEPLOY_ROOT").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    BaseDirs::new()
        .map(|base| base.data_local_dir().join("wisp"))
        .ok_or(DeployError::MissingDataDirectory)
}

fn wezterm_config_dir() -> Result<PathBuf, DeployError> {
    let override_dir = env::var_os("WISP_WEZTERM_CONFIG_DIR").filter(|value| !value.is_empty());
    let config_file = env::var_os("WEZTERM_CONFIG_FILE").filter(|value| !value.is_empty());
    let xdg_config = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty());
    let base = BaseDirs::new();
    resolve_wezterm_config_dir(
        override_dir.as_deref(),
        config_file.as_deref(),
        xdg_config.as_deref(),
        base.as_ref().map(BaseDirs::home_dir),
    )
    .ok_or(DeployError::MissingConfigDirectory)
}

fn resolve_wezterm_config_dir(
    override_dir: Option<&OsStr>,
    config_file: Option<&OsStr>,
    xdg_config_home: Option<&OsStr>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = override_dir {
        return Some(PathBuf::from(path));
    }
    if let Some(path) = config_file {
        let path = Path::new(path);
        return Some(
            path.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
        );
    }
    let home = home?;
    if home.join(".wezterm.lua").is_file() {
        return Some(home.to_path_buf());
    }
    if let Some(path) = xdg_config_home {
        return Some(PathBuf::from(path).join("wezterm"));
    }
    let dot_config = home.join(".config/wezterm");
    if dot_config.join("wezterm.lua").is_file() {
        return Some(dot_config);
    }
    Some(home.to_path_buf())
}

fn opencode_config_dir() -> Result<PathBuf, DeployError> {
    if let Some(path) = env::var_os("WISP_OPENCODE_CONFIG_DIR").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path).join("opencode"));
    }
    BaseDirs::new()
        .map(|base| base.home_dir().join(".config/opencode"))
        .ok_or(DeployError::MissingConfigDirectory)
}

fn bundle_id(assets: &[(&str, &[u8])]) -> String {
    let mut hasher = blake3::Hasher::new();
    let deployment_token = format!("wisp-deployment-v{PROTOCOL_VERSION}\0");
    hasher.update(deployment_token.as_bytes());
    for (path, contents) in assets {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(&(contents.len() as u64).to_le_bytes());
        hasher.update(contents);
    }
    hasher.finalize().to_hex().to_string()
}

fn verify_bundle(path: &Path, expected_id: &str) -> Result<Manifest, DeployError> {
    let manifest_path = path.join("manifest.json");
    let encoded = fs::read(&manifest_path)?;
    let version: serde_json::Value = serde_json::from_slice(&encoded)?;
    if version
        .get("deployment_schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(DEPLOYMENT_SCHEMA_VERSION.into())
    {
        return Err(DeployError::Invalid(format!(
            "unsupported manifest version in {}",
            manifest_path.display()
        )));
    }
    let manifest: Manifest = serde_json::from_slice(&encoded)?;
    if manifest.bundle_id != expected_id {
        return Err(DeployError::Invalid(format!(
            "manifest identity mismatch in {}",
            manifest_path.display()
        )));
    }
    let executable = if cfg!(windows) {
        "bin/wisp.exe"
    } else {
        "bin/wisp"
    };
    let expected_paths = [
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
    if manifest.files.len() != expected_paths.len()
        || expected_paths
            .iter()
            .any(|relative| !manifest.files.contains_key(*relative))
    {
        return Err(DeployError::Invalid(format!(
            "manifest file set mismatch in {}",
            manifest_path.display()
        )));
    }
    let mut contents = Vec::new();
    for relative in expected_paths {
        let file = fs::read(path.join(relative))?;
        let expected_hash = &manifest.files[relative];
        if blake3::hash(&file).to_hex().as_str() != expected_hash {
            return Err(DeployError::Invalid(format!(
                "checksum mismatch for {relative}"
            )));
        }
        contents.push((relative, file));
    }
    let assets = contents
        .iter()
        .map(|(relative, contents)| (*relative, contents.as_slice()))
        .collect::<Vec<_>>();
    if bundle_id(&assets) != expected_id {
        return Err(DeployError::Invalid(format!(
            "bundle ID mismatch in {}",
            manifest_path.display()
        )));
    }
    verify_executable_permissions(path.join(executable))?;
    Ok(manifest)
}

fn verify_compatible_bundle(path: &Path, expected_id: &str) -> Result<Manifest, DeployError> {
    let manifest = verify_bundle(path, expected_id)?;
    if manifest.protocol_version != PROTOCOL_VERSION
        || manifest.package_version != env!("CARGO_PKG_VERSION")
    {
        return Err(DeployError::Invalid(format!(
            "manifest compatibility mismatch in {}",
            path.join("manifest.json").display()
        )));
    }
    Ok(manifest)
}

fn read_active(root: &Path) -> Result<Option<ActiveDeployment>, DeployError> {
    let path = root.join("active.json");
    let encoded = match fs::read(&path) {
        Ok(encoded) => encoded,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let version: serde_json::Value = serde_json::from_slice(&encoded)?;
    if version
        .get("deployment_schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(DEPLOYMENT_SCHEMA_VERSION.into())
    {
        return Err(DeployError::IncompatibleActive { path });
    }
    let active: ActiveDeployment = serde_json::from_slice(&encoded)?;
    validate_bundle_id(&active.current_bundle_id)?;
    if let Some(previous) = &active.previous_bundle_id {
        validate_bundle_id(previous)?;
    }
    Ok(Some(active))
}

fn validate_bundle_id(bundle_id: &str) -> Result<(), DeployError> {
    if bundle_id.len() == 64
        && bundle_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Ok(());
    }
    Err(DeployError::Invalid("invalid bundle ID".into()))
}

fn write_stable_loaders(
    root: &Path,
    wezterm_config: &Path,
    executable_name: &str,
) -> Result<(), DeployError> {
    let (wezterm_loader, nvim_loader) = stable_loader_contents(root, executable_name)?;
    write_atomic(
        wezterm_config.join("wisp/init.lua"),
        wezterm_loader.as_bytes(),
    )?;
    write_atomic(root.join("nvim/lua/wisp/init.lua"), nvim_loader.as_bytes())?;
    write_atomic(root.join("nvim/doc/wisp.txt"), NVIM_HELP)?;
    Ok(())
}

fn stable_loader_contents(
    root: &Path,
    executable_name: &str,
) -> Result<(String, String), DeployError> {
    let root = serde_json::to_string(&root.to_string_lossy())?;
    let executable = serde_json::to_string(executable_name)?;
    let wezterm_loader = format!(
        r#"local wezterm = require "wezterm"
local root = {root}
local executable = {executable}

local function read_json(path)
  local file = assert(io.open(path, "rb"))
  local encoded = file:read "*a"
  file:close()
  return wezterm.json_parse(encoded)
end

local active = read_json(root .. "/active.json")
assert(active.deployment_schema_version == {DEPLOYMENT_SCHEMA_VERSION}, "unsupported Wisp deployment version")
assert(type(active.current_bundle_id) == "string" and active.current_bundle_id:match "^[0-9a-f]+$" and #active.current_bundle_id == 64, "invalid Wisp bundle ID")
local bundle = root .. "/deployments/" .. active.current_bundle_id
local manifest = read_json(bundle .. "/manifest.json")
assert(manifest.deployment_schema_version == {DEPLOYMENT_SCHEMA_VERSION}, "unsupported Wisp manifest version")
assert(manifest.bundle_id == active.current_bundle_id, "Wisp manifest bundle mismatch")
assert(manifest.protocol_version == {PROTOCOL_VERSION}, "unsupported Wisp protocol version")
local binary = bundle .. "/bin/" .. executable
local success, _, stderr = wezterm.run_child_process {{
  binary,
  "deploy",
  "check-bundle",
  root,
  active.current_bundle_id,
}}
assert(success, "Wisp bundle verification failed: " .. (stderr or "unknown error"))
local chunk = assert(loadfile(bundle .. "/wezterm/init.lua"))
return chunk(binary, "wisp-deployment-v{PROTOCOL_VERSION}", bundle .. "/wezterm")
"#
    );
    let nvim_loader = format!(
        r#"local root = {root}
local executable = {executable}

local function read_json(path)
  local file = assert(io.open(path, "rb"))
  local encoded = file:read "*a"
  file:close()
  return vim.json.decode(encoded)
end

local active = read_json(root .. "/active.json")
assert(active.deployment_schema_version == {DEPLOYMENT_SCHEMA_VERSION}, "unsupported Wisp deployment version")
assert(type(active.current_bundle_id) == "string" and active.current_bundle_id:match "^[0-9a-f]+$" and #active.current_bundle_id == 64, "invalid Wisp bundle ID")
local bundle = root .. "/deployments/" .. active.current_bundle_id
local manifest = read_json(bundle .. "/manifest.json")
assert(manifest.deployment_schema_version == {DEPLOYMENT_SCHEMA_VERSION}, "unsupported Wisp manifest version")
assert(manifest.bundle_id == active.current_bundle_id, "Wisp manifest bundle mismatch")
assert(manifest.protocol_version == {PROTOCOL_VERSION}, "unsupported Wisp protocol version")
local binary = bundle .. "/bin/" .. executable
local verification = vim.system({{
  binary,
  "deploy",
  "check-bundle",
  root,
  active.current_bundle_id,
}}, {{ text = true }}):wait()
assert(verification.code == 0, "Wisp bundle verification failed: " .. (verification.stderr or "unknown error"))
local chunk = assert(loadfile(bundle .. "/nvim/lua/wisp/init.lua"))
return chunk(binary, "wisp-deployment-v{PROTOCOL_VERSION}")
"#
    );
    Ok((wezterm_loader, nvim_loader))
}

fn opencode_loader_contents(root: &Path, executable_name: &str) -> Result<String, DeployError> {
    let root = serde_json::to_string(&root.to_string_lossy())?;
    let executable = serde_json::to_string(executable_name)?;
    Ok(format!(
        r#"import {{ readFileSync }} from "node:fs"
import path from "node:path"
import {{ spawnSync }} from "node:child_process"
import {{ pathToFileURL }} from "node:url"

const root = {root}
const executable = {executable}

function readJSON(file) {{
  return JSON.parse(readFileSync(file, "utf8"))
}}

export default async function WispPlugin(input) {{
  const active = readJSON(path.join(root, "active.json"))
  if (active.deployment_schema_version !== {DEPLOYMENT_SCHEMA_VERSION}) throw new Error("unsupported Wisp deployment version")
  if (typeof active.current_bundle_id !== "string" || !/^[0-9a-f]{{64}}$/.test(active.current_bundle_id)) throw new Error("invalid Wisp bundle ID")
  const bundle = path.join(root, "deployments", active.current_bundle_id)
  const manifest = readJSON(path.join(bundle, "manifest.json"))
  if (manifest.deployment_schema_version !== {DEPLOYMENT_SCHEMA_VERSION}) throw new Error("unsupported Wisp manifest version")
  if (manifest.bundle_id !== active.current_bundle_id) throw new Error("Wisp manifest bundle mismatch")
  if (manifest.protocol_version !== {PROTOCOL_VERSION}) throw new Error("unsupported Wisp protocol version")
  const binary = path.join(bundle, "bin", executable)
  const verified = spawnSync(binary, ["deploy", "check-bundle", root, active.current_bundle_id], {{
    shell: false,
    stdio: "pipe",
    windowsHide: true,
  }})
  if (verified.status !== 0) throw new Error(`Wisp bundle verification failed: ${{verified.stderr?.toString() || "unknown error"}}`)
  const plugin = await import(`${{pathToFileURL(path.join(bundle, "opencode/wisp.js")).href}}?bundle=${{active.current_bundle_id}}`)
  return plugin.default(input)
}}
"#
    ))
}

fn verify_stable_loaders(
    root: &Path,
    wezterm_config: &Path,
    executable_name: &str,
) -> Result<(), DeployError> {
    let (wezterm_loader, nvim_loader) = stable_loader_contents(root, executable_name)?;
    let expected = [
        (
            wezterm_config.join("wisp/init.lua"),
            wezterm_loader.as_bytes(),
        ),
        (root.join("nvim/lua/wisp/init.lua"), nvim_loader.as_bytes()),
        (root.join("nvim/doc/wisp.txt"), NVIM_HELP),
    ];
    for (path, contents) in expected {
        if fs::read(&path).ok().as_deref() != Some(contents) {
            return Err(DeployError::Invalid(format!(
                "stable host loader mismatch at {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn executable_name() -> &'static str {
    if cfg!(windows) { "wisp.exe" } else { "wisp" }
}

fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<(), DeployError> {
    let path = path.as_ref();
    let mut writer = BufWriter::new(fs::File::create(path)?);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn write_json_atomic(path: PathBuf, value: &impl Serialize) -> Result<(), DeployError> {
    let encoded = serde_json::to_vec_pretty(value)?;
    let mut contents = encoded;
    contents.push(b'\n');
    write_atomic(path, &contents)
}

fn write_atomic(path: PathBuf, contents: &[u8]) -> Result<(), DeployError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(contents)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), DeployError> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn verify_executable_permissions(path: PathBuf) -> Result<(), DeployError> {
    use std::os::unix::fs::PermissionsExt;

    if fs::metadata(&path)?.permissions().mode() & 0o111 == 0 {
        return Err(DeployError::Invalid(format!(
            "deployed executable is not executable: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), DeployError> {
    Ok(())
}

#[cfg(not(unix))]
fn verify_executable_permissions(_path: PathBuf) -> Result<(), DeployError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsStr, fs};

    use tempfile::TempDir;

    use super::resolve_wezterm_config_dir;

    #[test]
    fn wezterm_config_resolution_follows_explicit_environment_precedence() {
        let temporary = TempDir::new().unwrap();
        let home = temporary.path().join("home");
        let xdg = temporary.path().join("xdg");
        let configured = temporary.path().join("custom/wezterm.lua");
        let override_dir = temporary.path().join("override");

        assert_eq!(
            resolve_wezterm_config_dir(
                Some(override_dir.as_os_str()),
                Some(configured.as_os_str()),
                Some(xdg.as_os_str()),
                Some(&home),
            ),
            Some(override_dir)
        );
        assert_eq!(
            resolve_wezterm_config_dir(
                None,
                Some(configured.as_os_str()),
                Some(xdg.as_os_str()),
                Some(&home),
            ),
            Some(temporary.path().join("custom"))
        );
        assert_eq!(
            resolve_wezterm_config_dir(None, None, Some(xdg.as_os_str()), Some(&home)),
            Some(xdg.join("wezterm"))
        );
    }

    #[test]
    fn wezterm_config_resolution_uses_the_config_file_that_wezterm_will_load() {
        let temporary = TempDir::new().unwrap();
        let home = temporary.path().join("home");
        let dot_config = home.join(".config/wezterm");
        fs::create_dir_all(&dot_config).unwrap();

        assert_eq!(
            resolve_wezterm_config_dir(None, None, None, Some(&home)),
            Some(home.clone()),
            "an unused .config directory must not override a home .wezterm.lua"
        );

        fs::write(dot_config.join("wezterm.lua"), "return {}\n").unwrap();
        fs::write(home.join(".wezterm.lua"), "return {}\n").unwrap();
        assert_eq!(
            resolve_wezterm_config_dir(None, None, None, Some(&home)),
            Some(home.clone()),
            "WezTerm prefers its home config when both default files exist"
        );

        fs::remove_file(home.join(".wezterm.lua")).unwrap();
        assert_eq!(
            resolve_wezterm_config_dir(None, None, None, Some(&home)),
            Some(dot_config)
        );
        assert_eq!(
            resolve_wezterm_config_dir(None, Some(OsStr::new("wezterm.lua")), None, Some(&home)),
            Some(".".into())
        );
    }
}
