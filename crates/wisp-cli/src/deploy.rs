use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

use directories::BaseDirs;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use tempfile::{Builder, NamedTempFile};
use thiserror::Error;
use wisp_core::protocol::PROTOCOL_VERSION;

const DEPLOYMENT_SCHEMA_VERSION: u32 = 1;
const WEZTERM_ADAPTER: &[u8] = include_bytes!("../../../wezterm/init.lua");
const NVIM_ADAPTER: &[u8] = include_bytes!("../../../nvim/lua/wisp/init.lua");
const NVIM_HELP: &[u8] = include_bytes!("../../../nvim/doc/wisp.txt");

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

pub fn deploy() -> Result<PathBuf, DeployError> {
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
        ("nvim/lua/wisp/init.lua", NVIM_ADAPTER),
        ("nvim/doc/wisp.txt", NVIM_HELP),
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

    let previous = read_active(&root)?.and_then(|active| {
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
    for entry in fs::read_dir(&deployments)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !keep.contains(&name) {
            fs::remove_dir_all(entry.path())?;
            removed += 1;
        }
    }
    println!("pruned {removed} Wisp deployment(s)");
    Ok(())
}

pub fn check_bundle(root: &Path, bundle_id: &str) -> Result<(), DeployError> {
    validate_bundle_id(bundle_id)?;
    let _lock = DeploymentLock::acquire_shared(root)?;
    verify_compatible_bundle(&root.join("deployments").join(bundle_id), bundle_id)?;
    Ok(())
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
    if let Some(path) = env::var_os("WISP_WEZTERM_CONFIG_DIR").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path).join("wezterm"));
    }
    let base = BaseDirs::new().ok_or(DeployError::MissingConfigDirectory)?;
    let dot_config = base.home_dir().join(".config/wezterm");
    if dot_config.is_dir() {
        return Ok(dot_config);
    }
    Ok(base.config_dir().join("wezterm"))
}

fn bundle_id(assets: &[(&str, &[u8])]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"wisp-deployment-v1\0");
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
        "nvim/lua/wisp/init.lua",
        "nvim/doc/wisp.txt",
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
        return Err(DeployError::Invalid(format!(
            "unsupported active deployment version in {}",
            path.display()
        )));
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
return chunk(binary, "wisp-deployment-v1")
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
return chunk(binary, "wisp-deployment-v1")
"#
    );
    Ok((wezterm_loader, nvim_loader))
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
