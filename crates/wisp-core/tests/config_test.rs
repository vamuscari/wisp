use std::path::Path;

use wisp_core::config::{Config, ConfigError};

#[test]
fn parses_shared_config_and_expands_home_paths() {
    let config = Config::parse(
        r#"
version = 1

[[roots]]
path = "~/Repos"
group = "Code"

[[projects]]
id = "artifacts"
path = "~/Artifacts"
group = "Home"
name = "Artifacts"
display_name = "Personal artifacts"

[openers]
file = ["nvim", "{path}"]
"#,
        Path::new("/Users/test"),
    )
    .expect("config should parse");

    assert_eq!(config.version, 1);
    assert_eq!(config.cache_ttl_seconds, 60);
    assert!(!config.follow_symlinks);
    assert_eq!(config.roots[0].path, Path::new("/Users/test/Repos"));
    assert_eq!(config.roots[0].group.as_deref(), Some("Code"));
    assert_eq!(config.projects[0].id.as_deref(), Some("artifacts"));
    assert_eq!(config.projects[0].path, Path::new("/Users/test/Artifacts"));
    assert_eq!(
        config.projects[0].display_name.as_deref(),
        Some("Personal artifacts")
    );
    assert_eq!(
        config.openers.file,
        Some(vec!["nvim".into(), "{path}".into()])
    );
}

#[test]
fn rejects_unsupported_versions_and_shell_string_openers() {
    let version_error = Config::parse("version = 2", Path::new("/home/test"))
        .expect_err("unsupported versions should fail");
    assert!(matches!(version_error, ConfigError::UnsupportedVersion(2)));

    let opener_error = Config::parse(
        r#"
version = 1
[openers]
file = "nvim {path}"
"#,
        Path::new("/home/test"),
    )
    .expect_err("shell string openers should fail");
    assert!(
        opener_error.to_string().contains("openers.file"),
        "unexpected error: {opener_error}"
    );
}

#[test]
fn rejects_empty_paths_and_opener_arguments() {
    let root_error = Config::parse(
        r#"
version = 1
[[roots]]
path = ""
"#,
        Path::new("/home/test"),
    )
    .expect_err("empty roots should fail");
    assert!(root_error.to_string().contains("roots[0].path"));

    let opener_error = Config::parse(
        r#"
version = 1
[openers]
file = ["nvim", ""]
"#,
        Path::new("/home/test"),
    )
    .expect_err("empty opener arguments should fail");
    assert!(opener_error.to_string().contains("openers.file[1]"));
}
