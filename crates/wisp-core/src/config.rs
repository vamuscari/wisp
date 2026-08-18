use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::protocol::PROTOCOL_VERSION;

pub const CONFIG_VERSION: u32 = PROTOCOL_VERSION;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Config {
    pub version: u32,
    pub cache_ttl_seconds: u64,
    pub follow_symlinks: bool,
    pub roots: Vec<RootConfig>,
    pub projects: Vec<ProjectConfig>,
    pub openers: Openers,
    pub opencode: Option<OpenCodeConfig>,
    pub vcs: VcsConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RootConfig {
    pub path: PathBuf,
    pub group: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProjectConfig {
    pub id: Option<String>,
    pub path: PathBuf,
    pub group: Option<String>,
    pub name: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Openers {
    pub file: Option<Vec<String>>,
    pub project: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenCodeConfig {
    pub server_url: String,
    pub command: Vec<String>,
    pub session_limit: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct VcsConfig {
    pub icons: VcsIcons,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VcsIcons {
    pub clean: Option<String>,
    pub dirty: Option<String>,
    pub untracked: Option<String>,
    pub modified: Option<String>,
    pub staged: Option<String>,
    pub conflicted: Option<String>,
    pub ahead: Option<String>,
    pub behind: Option<String>,
    pub diverged: Option<String>,
    pub stashed: Option<String>,
}

impl Default for VcsIcons {
    fn default() -> Self {
        Self {
            clean: Some("✓".into()),
            dirty: Some("✗".into()),
            untracked: Some("?".into()),
            modified: Some("!".into()),
            staged: Some("+".into()),
            conflicted: Some("×".into()),
            ahead: Some("⇡".into()),
            behind: Some("⇣".into()),
            diverged: Some("⇕".into()),
            stashed: Some("*".into()),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to parse Wisp TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("unsupported Wisp config version {0}")]
    UnsupportedVersion(u32),
    #[error("invalid Wisp config: {0}")]
    Validation(String),
}

#[derive(Deserialize)]
struct VersionHeader {
    version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    #[serde(default = "default_cache_ttl")]
    cache_ttl_seconds: u64,
    #[serde(default)]
    follow_symlinks: bool,
    #[serde(default)]
    roots: Vec<RawRootConfig>,
    #[serde(default)]
    projects: Vec<RawProjectConfig>,
    #[serde(default)]
    openers: RawOpeners,
    opencode: Option<RawOpenCodeConfig>,
    #[serde(default)]
    vcs: RawVcsConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRootConfig {
    path: String,
    group: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProjectConfig {
    id: Option<String>,
    path: String,
    group: Option<String>,
    name: Option<String>,
    display_name: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOpeners {
    file: Option<toml::Value>,
    project: Option<toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOpenCodeConfig {
    server_url: String,
    command: Option<toml::Value>,
    #[serde(default = "default_opencode_session_limit")]
    session_limit: usize,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVcsConfig {
    #[serde(default)]
    icons: RawVcsIcons,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVcsIcons {
    clean: Option<toml::Value>,
    dirty: Option<toml::Value>,
    untracked: Option<toml::Value>,
    modified: Option<toml::Value>,
    staged: Option<toml::Value>,
    conflicted: Option<toml::Value>,
    ahead: Option<toml::Value>,
    behind: Option<toml::Value>,
    diverged: Option<toml::Value>,
    stashed: Option<toml::Value>,
}

const fn default_cache_ttl() -> u64 {
    60
}

const fn default_opencode_session_limit() -> usize {
    100
}

impl Config {
    pub fn parse(input: &str, home: &Path) -> Result<Self, ConfigError> {
        let header: VersionHeader = toml::from_str(input)?;
        if header.version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion(header.version));
        }
        let raw: RawConfig = toml::from_str(input)?;

        let roots = raw
            .roots
            .into_iter()
            .enumerate()
            .map(|(index, root)| {
                Ok(RootConfig {
                    path: validated_path(&root.path, home, &format!("roots[{index}].path"))?,
                    group: validated_optional_text(root.group, &format!("roots[{index}].group"))?,
                })
            })
            .collect::<Result<Vec<_>, ConfigError>>()?;

        let projects = raw
            .projects
            .into_iter()
            .enumerate()
            .map(|(index, project)| {
                Ok(ProjectConfig {
                    id: validated_optional_text(project.id, &format!("projects[{index}].id"))?,
                    path: validated_path(&project.path, home, &format!("projects[{index}].path"))?,
                    group: validated_optional_text(
                        project.group,
                        &format!("projects[{index}].group"),
                    )?,
                    name: validated_optional_text(
                        project.name,
                        &format!("projects[{index}].name"),
                    )?,
                    display_name: validated_optional_text(
                        project.display_name,
                        &format!("projects[{index}].display_name"),
                    )?,
                })
            })
            .collect::<Result<Vec<_>, ConfigError>>()?;

        let openers = Openers {
            file: validated_opener(raw.openers.file, "openers.file")?,
            project: validated_opener(raw.openers.project, "openers.project")?,
        };

        let opencode = raw
            .opencode
            .map(|opencode| {
                if opencode.session_limit == 0 {
                    return Err(ConfigError::Validation(
                        "opencode.session_limit must be greater than zero".into(),
                    ));
                }
                Ok(OpenCodeConfig {
                    server_url: validated_loopback_url(&opencode.server_url)?,
                    command: match opencode.command {
                        Some(command) => validated_argv(command, "opencode.command")?,
                        None => vec!["opencode".into()],
                    },
                    session_limit: opencode.session_limit,
                })
            })
            .transpose()?;

        let defaults = VcsIcons::default();
        let vcs = VcsConfig {
            icons: VcsIcons {
                clean: validated_vcs_icon(raw.vcs.icons.clean, defaults.clean, "vcs.icons.clean")?,
                dirty: validated_vcs_icon(raw.vcs.icons.dirty, defaults.dirty, "vcs.icons.dirty")?,
                untracked: validated_vcs_icon(
                    raw.vcs.icons.untracked,
                    defaults.untracked,
                    "vcs.icons.untracked",
                )?,
                modified: validated_vcs_icon(
                    raw.vcs.icons.modified,
                    defaults.modified,
                    "vcs.icons.modified",
                )?,
                staged: validated_vcs_icon(
                    raw.vcs.icons.staged,
                    defaults.staged,
                    "vcs.icons.staged",
                )?,
                conflicted: validated_vcs_icon(
                    raw.vcs.icons.conflicted,
                    defaults.conflicted,
                    "vcs.icons.conflicted",
                )?,
                ahead: validated_vcs_icon(raw.vcs.icons.ahead, defaults.ahead, "vcs.icons.ahead")?,
                behind: validated_vcs_icon(
                    raw.vcs.icons.behind,
                    defaults.behind,
                    "vcs.icons.behind",
                )?,
                diverged: validated_vcs_icon(
                    raw.vcs.icons.diverged,
                    defaults.diverged,
                    "vcs.icons.diverged",
                )?,
                stashed: validated_vcs_icon(
                    raw.vcs.icons.stashed,
                    defaults.stashed,
                    "vcs.icons.stashed",
                )?,
            },
        };

        Ok(Self {
            version: raw.version,
            cache_ttl_seconds: raw.cache_ttl_seconds,
            follow_symlinks: raw.follow_symlinks,
            roots,
            projects,
            openers,
            opencode,
            vcs,
        })
    }

    pub fn fingerprint(&self) -> String {
        let encoded = serde_json::to_vec(self).expect("serializing Config cannot fail");
        blake3::hash(&encoded).to_hex().to_string()
    }
}

fn validated_path(value: &str, home: &Path, field: &str) -> Result<PathBuf, ConfigError> {
    if value.is_empty() {
        return Err(ConfigError::Validation(format!(
            "{field} must not be empty"
        )));
    }
    if value == "~" {
        return Ok(home.to_path_buf());
    }
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return Ok(home.join(rest));
    }
    Ok(PathBuf::from(value))
}

fn validated_optional_text(
    value: Option<String>,
    field: &str,
) -> Result<Option<String>, ConfigError> {
    if value.as_deref() == Some("") {
        return Err(ConfigError::Validation(format!(
            "{field} must not be empty"
        )));
    }
    Ok(value)
}

fn validated_opener(
    value: Option<toml::Value>,
    field: &str,
) -> Result<Option<Vec<String>>, ConfigError> {
    let Some(value) = value else {
        return Ok(None);
    };
    validated_argv(value, field).map(Some)
}

fn validated_vcs_icon(
    value: Option<toml::Value>,
    default: Option<String>,
    field: &str,
) -> Result<Option<String>, ConfigError> {
    match value {
        None => Ok(default),
        Some(toml::Value::Boolean(false)) => Ok(None),
        Some(toml::Value::String(value)) if !value.is_empty() => Ok(Some(value)),
        Some(_) => Err(ConfigError::Validation(format!(
            "{field} must be a non-empty string or false"
        ))),
    }
}

fn validated_argv(value: toml::Value, field: &str) -> Result<Vec<String>, ConfigError> {
    let toml::Value::Array(values) = value else {
        return Err(ConfigError::Validation(format!(
            "{field} must be an argv array"
        )));
    };
    if values.is_empty() {
        return Err(ConfigError::Validation(format!(
            "{field} must not be empty"
        )));
    }

    let mut args = Vec::with_capacity(values.len());
    for (index, value) in values.into_iter().enumerate() {
        let toml::Value::String(value) = value else {
            return Err(ConfigError::Validation(format!(
                "{field}[{index}] must be a string"
            )));
        };
        if value.is_empty() {
            return Err(ConfigError::Validation(format!(
                "{field}[{index}] must not be empty"
            )));
        }
        args.push(value);
    }
    Ok(args)
}

fn validated_loopback_url(value: &str) -> Result<String, ConfigError> {
    let Some(authority) = value.strip_prefix("http://") else {
        return Err(ConfigError::Validation(
            "opencode.server_url must be a loopback HTTP URL".into(),
        ));
    };
    let authority = authority.strip_suffix('/').unwrap_or(authority);
    if authority.is_empty()
        || authority.contains('/')
        || authority.contains('?')
        || authority.contains('#')
        || authority.contains('@')
    {
        return Err(ConfigError::Validation(
            "opencode.server_url must be a loopback HTTP URL".into(),
        ));
    }

    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, remainder)) = rest.split_once(']') else {
            return Err(ConfigError::Validation(
                "opencode.server_url must be a loopback HTTP URL".into(),
            ));
        };
        let port = remainder
            .strip_prefix(':')
            .filter(|_| !remainder.is_empty());
        if !remainder.is_empty() && port.is_none() {
            return Err(ConfigError::Validation(
                "opencode.server_url must be a loopback HTTP URL".into(),
            ));
        }
        (host, port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        (host, Some(port))
    } else {
        (authority, None)
    };

    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    let valid_port = port.is_none_or(|port| port.parse::<u16>().is_ok_and(|port| port > 0));
    if !loopback || !valid_port {
        return Err(ConfigError::Validation(
            "opencode.server_url must be a loopback HTTP URL".into(),
        ));
    }

    Ok(value.trim_end_matches('/').to_string())
}
