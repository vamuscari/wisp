use std::path::Path;

use wisp_core::{
    cache::CACHE_VERSION,
    config::{CONFIG_VERSION, Config, ConfigError},
    protocol::PROTOCOL_VERSION,
};

#[test]
fn persistent_schema_versions_match_the_protocol() {
    assert_eq!(CONFIG_VERSION, PROTOCOL_VERSION);
    assert_eq!(CACHE_VERSION, PROTOCOL_VERSION);
}

#[test]
fn parses_opencode_shared_server_config() {
    let config = Config::parse(
        r#"
version = 3

[opencode]
server_url = "http://127.0.0.1:4096"
command = ["opencode"]
session_limit = 25
"#,
        Path::new("/Users/test"),
    )
    .expect("OpenCode config should parse");

    let opencode = config.opencode.expect("OpenCode should be enabled");
    assert_eq!(opencode.server_url, "http://127.0.0.1:4096");
    assert_eq!(opencode.command, vec!["opencode"]);
    assert_eq!(opencode.session_limit, 25);
}

#[test]
fn opencode_config_defaults_command_and_limit() {
    let config = Config::parse(
        r#"
version = 3

[opencode]
server_url = "http://localhost:4096"
"#,
        Path::new("/Users/test"),
    )
    .expect("OpenCode defaults should be valid");

    let opencode = config.opencode.expect("OpenCode should be enabled");
    assert_eq!(opencode.command, vec!["opencode"]);
    assert_eq!(opencode.session_limit, 100);
}

#[test]
fn rejects_non_loopback_opencode_servers_and_invalid_limits() {
    for (server_url, expected) in [
        ("https://127.0.0.1:4096", "loopback HTTP URL"),
        ("http://example.com:4096", "loopback HTTP URL"),
    ] {
        let error = Config::parse(
            &format!(
                r#"
version = 3

[opencode]
server_url = "{server_url}"
"#
            ),
            Path::new("/Users/test"),
        )
        .expect_err("non-loopback OpenCode URLs should fail");
        assert!(
            error.to_string().contains(expected),
            "unexpected error: {error}"
        );
    }

    let limit_error = Config::parse(
        r#"
version = 3

[opencode]
server_url = "http://127.0.0.1:4096"
session_limit = 0
"#,
        Path::new("/Users/test"),
    )
    .expect_err("a zero session limit should fail");
    assert!(limit_error.to_string().contains("session_limit"));

    let command_error = Config::parse(
        r#"
version = 3

[opencode]
server_url = "http://127.0.0.1:4096"
command = []
"#,
        Path::new("/Users/test"),
    )
    .expect_err("an empty OpenCode command should fail");
    assert!(command_error.to_string().contains("opencode.command"));
}

#[test]
fn parses_shared_config_and_expands_home_paths() {
    let config = Config::parse(
        r#"
version = 3

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

    assert_eq!(config.version, 3);
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
        .expect_err("old versions should fail");
    assert!(matches!(version_error, ConfigError::UnsupportedVersion(2)));

    let future_error = Config::parse("version = 4\nfuture_option = true", Path::new("/home/test"))
        .expect_err("the version should be checked before future fields");
    assert!(matches!(future_error, ConfigError::UnsupportedVersion(4)));

    let opener_error = Config::parse(
        r#"
version = 3
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
version = 3
[[roots]]
path = ""
"#,
        Path::new("/home/test"),
    )
    .expect_err("empty roots should fail");
    assert!(root_error.to_string().contains("roots[0].path"));

    let opener_error = Config::parse(
        r#"
version = 3
[openers]
file = ["nvim", ""]
"#,
        Path::new("/home/test"),
    )
    .expect_err("empty opener arguments should fail");
    assert!(opener_error.to_string().contains("openers.file[1]"));
}
