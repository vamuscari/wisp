use std::{
    env,
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use directories::BaseDirs;
use tempfile::NamedTempFile;
use thiserror::Error;
use wisp_core::{
    cache::{CacheError, CacheStore},
    catalog::{Catalog, CatalogError},
    config::{Config, ConfigError},
    discovery::StdFileSystem,
    model::{DirectoryEntry, Project},
    path::{comparison_key, normalized_path},
    protocol::{
        HostContext, OpenCodeStatusEnvelope, ProjectsEnvelope, Selection, SelectionEnvelope,
        SelectionStatus,
    },
};
use wisp_tui::{ActiveProjectContext, App, DataSource, GitSummary, TuiError};

mod deploy;
pub mod opencode;

#[derive(Debug, Parser)]
#[command(
    name = "wisp",
    version,
    about = "Pick projects, OpenCode sessions, and files from the terminal"
)]
struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<WispCommand>,
}

#[derive(Debug, Subcommand)]
enum WispCommand {
    Pick(PickArgs),
    Projects {
        #[arg(long)]
        json: bool,
    },
    Refresh,
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Deploy {
        #[command(subcommand)]
        command: Option<DeployCommand>,
    },
    Open {
        #[arg(value_name = "SELECTION_JSON")]
        selection_json: String,
    },
    #[command(name = "opencode")]
    Opencode {
        #[command(subcommand)]
        command: OpenCodeCommand,
    },
}

#[derive(Clone, Debug, Default, Args)]
struct PickArgs {
    #[arg(long, value_name = "PATH")]
    result_file: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    host_context_file: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    active_project_path: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    active_file: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = InitialView::Projects)]
    initial_view: InitialView,
    #[arg(long)]
    disable_sessions: bool,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum InitialView {
    #[default]
    Projects,
    Windows,
    Sessions,
}

#[derive(Clone, Copy, Debug, Subcommand)]
enum CacheCommand {
    Clear,
}

#[derive(Clone, Copy, Debug, Subcommand)]
enum ConfigCommand {
    Validate,
}

#[derive(Clone, Debug, Subcommand)]
enum OpenCodeCommand {
    Install,
    Status {
        #[arg(long)]
        json: bool,
    },
    #[command(hide = true)]
    Register {
        #[arg(long)]
        server_url: String,
        #[arg(long)]
        directory: PathBuf,
        #[arg(long)]
        project_path: PathBuf,
        #[arg(long)]
        pid: u32,
        #[arg(long)]
        pane_id: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        session_status: Option<String>,
        #[arg(long, default_value_t = 0)]
        waiting_permissions: usize,
        #[arg(long, default_value_t = 0)]
        waiting_questions: usize,
        #[arg(long)]
        session_error: Option<String>,
    },
    #[command(hide = true)]
    Unregister {
        #[arg(long)]
        directory: PathBuf,
        #[arg(long)]
        pid: u32,
    },
}

#[derive(Clone, Debug, Subcommand)]
enum DeployCommand {
    Verify,
    Status {
        #[arg(long)]
        json: bool,
    },
    Prune,
    #[command(hide = true)]
    CheckBundle {
        root: PathBuf,
        bundle_id: String,
    },
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("could not determine the home directory")]
    MissingHome,
    #[error("could not determine the platform configuration directory")]
    MissingConfigDirectory,
    #[error("could not determine the platform cache directory")]
    MissingCacheDirectory,
    #[error("failed to read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("invalid configuration in {path}: {source}")]
    Config { path: PathBuf, source: ConfigError },
    #[error("invalid host context in {path}: {source}")]
    HostContext {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("invalid selection JSON: {0}")]
    SelectionJson(serde_json::Error),
    #[error("wisp open expected a selected result")]
    NotSelected,
    #[error("the selection has no opener")]
    MissingOpener,
    #[error("the selection opener is empty")]
    EmptyOpener,
    #[error("failed to launch opener {program}: {source}")]
    Launch { program: String, source: io::Error },
    #[error("opener exited unsuccessfully with {0}")]
    OpenerFailed(String),
    #[error(transparent)]
    Cache(#[from] CacheError),
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error(transparent)]
    Tui(#[from] TuiError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Deploy(#[from] deploy::DeployError),
    #[error(transparent)]
    OpenCode(#[from] opencode::OpenCodeError),
}

pub fn run() -> i32 {
    let cli = Cli::parse();
    let command = cli
        .command
        .unwrap_or_else(|| WispCommand::Pick(PickArgs::default()));
    match command {
        WispCommand::Pick(args) => run_pick_command(cli.config.as_deref(), args),
        command => match run_noninteractive(cli.config.as_deref(), command) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("wisp: {error}");
                1
            }
        },
    }
}

fn run_pick_command(config_override: Option<&Path>, args: PickArgs) -> i32 {
    let result = pick(
        config_override,
        args.host_context_file.as_deref(),
        args.active_project_path.as_deref(),
        args.active_file.as_deref(),
        args.initial_view,
        !args.disable_sessions,
    );
    let (envelope, exit_code) = match result {
        Ok(Some(selection)) => (SelectionEnvelope::selected(selection), 0),
        Ok(None) => (SelectionEnvelope::cancelled(), 0),
        Err(error) => (SelectionEnvelope::error(error.to_string()), 1),
    };
    if let Err(error) = emit_envelope(&envelope, args.result_file.as_deref()) {
        eprintln!("wisp: failed to write selection result: {error}");
        return 1;
    }
    exit_code
}

fn run_noninteractive(
    config_override: Option<&Path>,
    command: WispCommand,
) -> Result<(), CliError> {
    match command {
        WispCommand::Projects { json } => {
            let (_, mut catalog) = load_catalog(config_override)?;
            let projects = catalog.projects(now())?;
            if json {
                let stdout = io::stdout();
                let mut writer = stdout.lock();
                serde_json::to_writer_pretty(&mut writer, &ProjectsEnvelope::new(projects))?;
                writer.write_all(b"\n")?;
            } else {
                for project in projects {
                    println!("{}\t{}", project.display_name, project.path.display());
                }
            }
            Ok(())
        }
        WispCommand::Refresh => {
            let (_, mut catalog) = load_catalog(config_override)?;
            let projects = catalog.refresh(now())?;
            println!("refreshed {} projects", projects.len());
            Ok(())
        }
        WispCommand::Cache {
            command: CacheCommand::Clear,
        } => {
            let config_path = config_path(config_override)?;
            let config = load_config(&config_path)?;
            let mut cache = CacheStore::open(cache_path()?, config.fingerprint())?;
            cache.clear();
            cache.save()?;
            println!("cache cleared");
            Ok(())
        }
        WispCommand::Config {
            command: ConfigCommand::Validate,
        } => {
            let path = config_path(config_override)?;
            load_config(&path)?;
            println!("configuration is valid: {}", path.display());
            Ok(())
        }
        WispCommand::Deploy { command: None } => deploy::deploy().map(|_| ()).map_err(Into::into),
        WispCommand::Deploy {
            command: Some(DeployCommand::Verify),
        } => deploy::verify().map_err(Into::into),
        WispCommand::Deploy {
            command: Some(DeployCommand::Status { json }),
        } => deploy::status(json).map_err(Into::into),
        WispCommand::Deploy {
            command: Some(DeployCommand::Prune),
        } => deploy::prune().map_err(Into::into),
        WispCommand::Deploy {
            command: Some(DeployCommand::CheckBundle { root, bundle_id }),
        } => deploy::check_bundle(&root, &bundle_id).map_err(Into::into),
        WispCommand::Open { selection_json } => open_selection(&selection_json),
        WispCommand::Opencode {
            command: OpenCodeCommand::Install,
        } => deploy::install_opencode().map(|_| ()).map_err(Into::into),
        WispCommand::Opencode {
            command: OpenCodeCommand::Status { json },
        } => {
            let registry = opencode::default_registry_dir()?;
            let sessions = opencode::live_status(&registry)?;
            if json {
                let stdout = io::stdout();
                let mut writer = stdout.lock();
                serde_json::to_writer_pretty(&mut writer, &OpenCodeStatusEnvelope::new(sessions))?;
                writer.write_all(b"\n")?;
            } else {
                println!(
                    "OpenCode sessions: wait {}  run {}  retry {}  idle {}  err {}",
                    sessions.waiting,
                    sessions.running,
                    sessions.retrying,
                    sessions.idle,
                    sessions.error
                );
            }
            Ok(())
        }
        WispCommand::Opencode {
            command:
                OpenCodeCommand::Register {
                    server_url,
                    directory,
                    project_path,
                    pid,
                    pane_id,
                    session_id,
                    session_status,
                    waiting_permissions,
                    waiting_questions,
                    session_error,
                },
        } => {
            let registry = opencode::default_registry_dir()?;
            let session_activity = session_status
                .as_deref()
                .map(opencode::decode_session_status)
                .transpose()?;
            opencode::register_instance(
                &registry,
                &opencode::RegistryRegistration {
                    server_url,
                    directory,
                    project_path,
                    pid,
                    pane_id,
                    session_id,
                    session_activity,
                    session_waiting: wisp_core::opencode::SessionWaiting {
                        permissions: waiting_permissions,
                        questions: waiting_questions,
                    },
                    session_error,
                },
            )?;
            Ok(())
        }
        WispCommand::Opencode {
            command: OpenCodeCommand::Unregister { directory, pid },
        } => {
            let registry = opencode::default_registry_dir()?;
            opencode::unregister_instance(&registry, pid, &directory)?;
            Ok(())
        }
        WispCommand::Pick(_) => unreachable!("pick is handled before noninteractive commands"),
    }
}

fn pick(
    config_override: Option<&Path>,
    host_context_path: Option<&Path>,
    active_project_path: Option<&Path>,
    active_file: Option<&Path>,
    initial_view: InitialView,
    sessions_enabled: bool,
) -> Result<Option<Selection>, CliError> {
    let (config, mut catalog) = load_catalog(config_override)?;
    let projects = catalog.projects(now())?;
    let context = host_context_path.map(read_host_context).transpose()?;
    let active_project = build_active_project_context(
        &projects,
        context.as_ref(),
        active_project_path,
        active_file,
    );
    let active_project_git = active_project.as_ref().and_then(|active| {
        projects
            .iter()
            .find(|project| project.id == active.project_id)
            .map(|project| spawn_git_summary(project.id.clone(), project.path.clone()))
    });
    let opencode_config = sessions_enabled.then(|| config.opencode.clone()).flatten();
    let vcs_icons = config.vcs.icons;
    let tui_initial_view = match initial_view {
        InitialView::Projects => wisp_tui::InitialView::Projects,
        InitialView::Windows => wisp_tui::InitialView::Windows,
        InitialView::Sessions => wisp_tui::InitialView::Sessions,
    };
    let mut app = match &opencode_config {
        Some(opencode) => App::new_with_opencode(
            projects,
            config.openers,
            config.follow_symlinks,
            context,
            tui_initial_view,
            opencode.command.clone(),
        ),
        None => App::new(
            projects,
            config.openers,
            config.follow_symlinks,
            context,
            tui_initial_view,
        ),
    };
    if let Some(active_project) = active_project {
        app.set_active_project_context(active_project);
    }
    app.set_vcs_icons(vcs_icons);
    let opencode = opencode_config
        .map(opencode::OpenCodeClient::new)
        .transpose()?
        .map(|client| OpenCodeDataSource {
            watcher: client.watch_shared(),
            client,
            last_poll: Instant::now(),
        });
    let mut data = CatalogDataSource {
        catalog: &mut catalog,
        active_project_git,
        opencode,
    };
    Ok(wisp_tui::run(app, &mut data)?)
}

fn resolve_active_project<'a>(
    projects: &'a [Project],
    context: Option<&HostContext>,
    active_project_path: Option<&Path>,
    active_file: Option<&Path>,
) -> Option<&'a Project> {
    if let Some(path) = active_project_path {
        let active_project_path = comparison_key(&path.to_string_lossy());
        if let Some(project) = projects
            .iter()
            .find(|project| comparison_key(&project.path.to_string_lossy()) == active_project_path)
        {
            return Some(project);
        }
    }
    if let Some(context) = context {
        if let Some(project) = projects.iter().find(|project| {
            context
                .labels(&project.id)
                .iter()
                .any(|label| label == "current")
        }) {
            return Some(project);
        }
    }
    let active_file = comparison_key(&active_file?.to_string_lossy());
    projects
        .iter()
        .filter(|project| {
            path_contains(
                &comparison_key(&project.path.to_string_lossy()),
                &active_file,
            )
        })
        .max_by_key(|project| comparison_key(&project.path.to_string_lossy()).len())
}

fn path_contains(directory: &str, candidate: &str) -> bool {
    candidate == directory
        || candidate
            .strip_prefix(directory)
            .is_some_and(|relative| directory.ends_with('/') || relative.starts_with('/'))
}

fn relative_project_file(project: &Project, file: &Path) -> Option<String> {
    let project_key = comparison_key(&project.path.to_string_lossy());
    let file_key = comparison_key(&file.to_string_lossy());
    if !path_contains(&project_key, &file_key) || project_key == file_key {
        return None;
    }
    if let Ok(relative) = file.strip_prefix(&project.path) {
        return Some(relative.to_string_lossy().replace('\\', "/"));
    }
    let relative = file_key.strip_prefix(&project_key)?.trim_start_matches('/');
    let normalized_file = normalized_path(&file.to_string_lossy());
    normalized_file
        .get(normalized_file.len().checked_sub(relative.len())?..)
        .map(str::to_owned)
}

fn parse_git_status(status: &str) -> Option<GitSummary> {
    let mut branch = None;
    let mut object_id = None;
    let mut dirty = false;
    let mut untracked = 0;
    let mut modified = 0;
    let mut staged = 0;
    let mut conflicted = 0;
    let mut ahead = 0;
    let mut behind = 0;
    let mut stashed = 0;
    for line in status.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            branch = Some(head.to_string());
        } else if let Some(oid) = line.strip_prefix("# branch.oid ") {
            object_id = Some(oid.to_string());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let mut counts = ab.split_whitespace();
            ahead = counts.next()?.strip_prefix('+')?.parse().ok()?;
            behind = counts.next()?.strip_prefix('-')?.parse().ok()?;
        } else if let Some(count) = line.strip_prefix("# stash ") {
            stashed = count.parse().ok()?;
        } else if line.starts_with("? ") {
            dirty = true;
            untracked += 1;
        } else if line.starts_with("u ") {
            dirty = true;
            conflicted += 1;
        } else if let Some(record) = line.strip_prefix("1 ").or_else(|| line.strip_prefix("2 ")) {
            dirty = true;
            let mut status = record.split_whitespace().next()?.chars();
            if status.next()? != '.' {
                staged += 1;
            }
            if status.next()? != '.' {
                modified += 1;
            }
        } else if !line.is_empty() && !line.starts_with("# ") {
            dirty = true;
        }
    }
    let branch = match branch?.as_str() {
        "(detached)" => format!("@{}", object_id?.chars().take(7).collect::<String>()),
        branch => branch.to_string(),
    };
    Some(GitSummary {
        branch,
        dirty,
        untracked,
        modified,
        staged,
        conflicted,
        ahead,
        behind,
        stashed,
    })
}

fn git_summary(project_path: &Path) -> Option<GitSummary> {
    let output = ProcessCommand::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(project_path)
        .args([
            "status",
            "--porcelain=v2",
            "--branch",
            "--show-stash",
            "--untracked-files=normal",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_git_status(&String::from_utf8_lossy(&output.stdout))
}

fn spawn_git_summary(project_id: String, project_path: PathBuf) -> Receiver<(String, GitSummary)> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        if let Some(summary) = git_summary(&project_path) {
            let _ = sender.send((project_id, summary));
        }
    });
    receiver
}

fn build_active_project_context(
    projects: &[Project],
    context: Option<&HostContext>,
    active_project_path: Option<&Path>,
    active_file: Option<&Path>,
) -> Option<ActiveProjectContext> {
    let project = resolve_active_project(projects, context, active_project_path, active_file)?;
    Some(ActiveProjectContext {
        project_id: project.id.clone(),
        file: active_file.and_then(|file| relative_project_file(project, file)),
        git: None,
    })
}

struct CatalogDataSource<'a> {
    catalog: &'a mut Catalog<StdFileSystem>,
    active_project_git: Option<Receiver<(String, GitSummary)>>,
    opencode: Option<OpenCodeDataSource>,
}

struct OpenCodeDataSource {
    client: opencode::OpenCodeClient,
    watcher: opencode::OpenCodeWatcher,
    last_poll: Instant,
}

impl DataSource for CatalogDataSource<'_> {
    fn active_project_git_update(&mut self) -> Option<(String, GitSummary)> {
        let update = self.active_project_git.as_ref()?.try_recv();
        match update {
            Ok(update) => {
                self.active_project_git = None;
                Some(update)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.active_project_git = None;
                None
            }
        }
    }

    fn directory(&mut self, path: &Path) -> Result<Vec<DirectoryEntry>, String> {
        self.catalog
            .directory(path, now())
            .map_err(|error| error.to_string())
    }

    fn refresh_projects(&mut self) -> Result<Vec<Project>, String> {
        self.catalog
            .refresh(now())
            .map_err(|error| error.to_string())
    }

    fn refresh_directory(&mut self, path: &Path) -> Result<Vec<DirectoryEntry>, String> {
        self.catalog
            .refresh_directory(path, now())
            .map_err(|error| error.to_string())
    }

    fn sessions(&mut self, path: &Path) -> Result<wisp_core::opencode::OpenCodeSnapshot, String> {
        let opencode = self
            .opencode
            .as_mut()
            .ok_or_else(|| "OpenCode integration is not configured".to_string())?;
        let snapshot = opencode.client.snapshot(path);
        opencode.last_poll = Instant::now();
        snapshot.map_err(|error| error.to_string())
    }

    fn refresh_sessions(
        &mut self,
        path: &Path,
    ) -> Result<wisp_core::opencode::OpenCodeSnapshot, String> {
        self.sessions(path)
    }

    fn session_updates_pending(&mut self) -> bool {
        let Some(opencode) = &mut self.opencode else {
            return false;
        };
        if opencode.watcher.changed() || opencode.last_poll.elapsed() >= Duration::from_secs(1) {
            return true;
        }
        false
    }
}

fn load_catalog(
    config_override: Option<&Path>,
) -> Result<(Config, Catalog<StdFileSystem>), CliError> {
    let path = config_path(config_override)?;
    let config = load_config(&path)?;
    let cache = CacheStore::open(cache_path()?, config.fingerprint())?;
    let catalog = Catalog::new(config.clone(), StdFileSystem, cache);
    Ok((config, catalog))
}

fn load_config(path: &Path) -> Result<Config, CliError> {
    let contents = fs::read_to_string(path).map_err(|source| CliError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    Config::parse(&contents, &home_dir()?).map_err(|source| CliError::Config {
        path: path.to_path_buf(),
        source,
    })
}

fn config_path(config_override: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(path) = config_override {
        return Ok(path.to_path_buf());
    }
    if let Some(path) = env::var_os("WISP_CONFIG_FILE").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path).join("wisp/config.toml"));
    }
    let base = BaseDirs::new().ok_or(CliError::MissingConfigDirectory)?;
    Ok(base.config_dir().join("wisp/config.toml"))
}

fn cache_path() -> Result<PathBuf, CliError> {
    if let Some(path) = env::var_os("WISP_CACHE_FILE").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = env::var_os("XDG_CACHE_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path).join("wisp/cache.json"));
    }
    let base = BaseDirs::new().ok_or(CliError::MissingCacheDirectory)?;
    Ok(base.cache_dir().join("wisp/cache.json"))
}

fn home_dir() -> Result<PathBuf, CliError> {
    if let Some(path) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    BaseDirs::new()
        .map(|base| base.home_dir().to_path_buf())
        .ok_or(CliError::MissingHome)
}

pub fn read_host_context(path: &Path) -> Result<HostContext, CliError> {
    let contents = fs::read(path).map_err(|source| CliError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&contents).map_err(|source| CliError::HostContext {
        path: path.to_path_buf(),
        source,
    })
}

fn open_selection(json: &str) -> Result<(), CliError> {
    let envelope: SelectionEnvelope =
        serde_json::from_str(json).map_err(CliError::SelectionJson)?;
    if envelope.status != SelectionStatus::Selected {
        return Err(CliError::NotSelected);
    }
    let selection = envelope.selection.ok_or(CliError::NotSelected)?;
    let opener = match selection {
        Selection::Project { opener, .. } | Selection::File { opener, .. } => {
            opener.ok_or(CliError::MissingOpener)?
        }
        Selection::OpenCodeSession { opener, .. } => opener,
        Selection::CloseProject { .. }
        | Selection::HostItem { .. }
        | Selection::Workspace { .. }
        | Selection::WorkspaceItem { .. }
        | Selection::CloseWorkspace { .. } => {
            return Err(CliError::MissingOpener);
        }
    };
    let (program, arguments) = opener.split_first().ok_or(CliError::EmptyOpener)?;
    let status = ProcessCommand::new(program)
        .args(arguments)
        .status()
        .map_err(|source| CliError::Launch {
            program: program.clone(),
            source,
        })?;
    if !status.success() {
        return Err(CliError::OpenerFailed(status.to_string()));
    }
    Ok(())
}

fn emit_envelope(envelope: &SelectionEnvelope, result_file: Option<&Path>) -> Result<(), CliError> {
    if let Some(path) = result_file {
        return write_envelope_file(path, envelope);
    }
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    serde_json::to_writer_pretty(&mut writer, envelope)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

pub fn write_envelope_file(path: &Path, envelope: &SelectionEnvelope) -> Result<(), CliError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        serde_json::to_writer_pretty(&mut writer, envelope)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    sync_parent(parent);
    Ok(())
}

fn sync_parent(parent: &Path) {
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use tempfile::TempDir;
    use wisp_core::config::OpenCodeConfig;

    use super::*;

    #[test]
    fn explicit_project_path_resolves_a_project_relative_active_file() {
        let projects = vec![Project {
            id: "api".into(),
            path: PathBuf::from("/repos/api"),
            group: "Repos".into(),
            name: "api".into(),
            display_name: "API".into(),
        }];

        let project = resolve_active_project(
            &projects,
            None,
            Some(Path::new("/repos/api")),
            Some(Path::new("/repos/api/src/main.rs")),
        )
        .unwrap();

        assert_eq!(project.id, "api");
        assert_eq!(
            relative_project_file(project, Path::new("/repos/api/src/main.rs")).as_deref(),
            Some("src/main.rs")
        );
    }

    #[test]
    fn active_file_infers_the_deepest_containing_project_without_host_context() {
        let projects = vec![
            Project {
                id: "repos".into(),
                path: PathBuf::from("/repos"),
                group: "Home".into(),
                name: "repos".into(),
                display_name: "Repos".into(),
            },
            Project {
                id: "api".into(),
                path: PathBuf::from("/repos/api"),
                group: "Repos".into(),
                name: "api".into(),
                display_name: "API".into(),
            },
        ];

        let project = resolve_active_project(
            &projects,
            None,
            None,
            Some(Path::new("/repos/api/src/main.rs")),
        )
        .unwrap();

        assert_eq!(project.id, "api");
    }

    #[test]
    fn project_relative_file_rejects_paths_that_escape_the_project() {
        let project = Project {
            id: "api".into(),
            path: PathBuf::from("/repos/api"),
            group: "Repos".into(),
            name: "api".into(),
            display_name: "API".into(),
        };

        assert_eq!(
            relative_project_file(&project, Path::new("/repos/api/../other/secrets.txt")),
            None
        );
    }

    #[test]
    fn project_relative_file_preserves_windows_filename_casing() {
        let project = Project {
            id: "api".into(),
            path: PathBuf::from(r"C:\Repos\Api"),
            group: "Repos".into(),
            name: "api".into(),
            display_name: "API".into(),
        };

        assert_eq!(
            relative_project_file(&project, Path::new(r"c:\repos\api\Src\Main.RS")).as_deref(),
            Some("Src/Main.RS")
        );
    }

    #[test]
    fn host_current_project_takes_precedence_over_file_inference() {
        let projects = vec![
            Project {
                id: "api".into(),
                path: PathBuf::from("/repos/api"),
                group: "Repos".into(),
                name: "api".into(),
                display_name: "API".into(),
            },
            Project {
                id: "web".into(),
                path: PathBuf::from("/repos/web"),
                group: "Repos".into(),
                name: "web".into(),
                display_name: "Web".into(),
            },
        ];
        let context: HostContext = serde_json::from_value(serde_json::json!({
            "protocol_version": 4,
            "projects": {
                "api": { "labels": ["open"] },
                "web": { "labels": ["current", "open"] }
            },
            "workspaces": {}
        }))
        .unwrap();

        let project = resolve_active_project(
            &projects,
            Some(&context),
            None,
            Some(Path::new("/repos/api/src/main.rs")),
        )
        .unwrap();

        assert_eq!(project.id, "web");
    }

    #[test]
    fn unmatched_explicit_project_falls_back_to_the_host_current_project() {
        let projects = vec![
            Project {
                id: "api".into(),
                path: PathBuf::from("/repos/api"),
                group: "Repos".into(),
                name: "api".into(),
                display_name: "API".into(),
            },
            Project {
                id: "web".into(),
                path: PathBuf::from("/repos/web"),
                group: "Repos".into(),
                name: "web".into(),
                display_name: "Web".into(),
            },
        ];
        let context: HostContext = serde_json::from_value(serde_json::json!({
            "protocol_version": 4,
            "projects": {
                "api": { "labels": ["open"] },
                "web": { "labels": ["current", "open"] }
            },
            "workspaces": {}
        }))
        .unwrap();

        let project = resolve_active_project(
            &projects,
            Some(&context),
            Some(Path::new("/repos/missing")),
            Some(Path::new("/repos/api/src/main.rs")),
        )
        .unwrap();

        assert_eq!(project.id, "web");
    }

    #[test]
    fn host_context_without_a_current_project_falls_back_to_the_active_file() {
        let projects = vec![
            Project {
                id: "repos".into(),
                path: PathBuf::from("/repos"),
                group: "Home".into(),
                name: "repos".into(),
                display_name: "Repos".into(),
            },
            Project {
                id: "api".into(),
                path: PathBuf::from("/repos/api"),
                group: "Repos".into(),
                name: "api".into(),
                display_name: "API".into(),
            },
        ];
        let context: HostContext = serde_json::from_value(serde_json::json!({
            "protocol_version": 4,
            "projects": {
                "repos": { "labels": ["open"] },
                "api": { "labels": ["new"] }
            },
            "workspaces": {
                "default": { "current": true }
            }
        }))
        .unwrap();

        let project = resolve_active_project(
            &projects,
            Some(&context),
            None,
            Some(Path::new("/repos/api/src/main.rs")),
        )
        .unwrap();

        assert_eq!(project.id, "api");
    }

    #[test]
    fn porcelain_v2_status_counts_worktree_upstream_and_stash_states() {
        let status = parse_git_status(
            "# branch.oid 1cf82045403b6911084598a4487b373dc341638e\n\
             # branch.head main\n\
             # branch.upstream origin/main\n\
             # branch.ab +2 -1\n\
             # stash 3\n\
             1 .M N... 100644 100644 100644 abcdef0 abcdef0 src/modified.rs\n\
             1 M. N... 100644 100644 100644 abcdef0 abcdef1 src/staged.rs\n\
             1 MM N... 100644 100644 100644 abcdef0 abcdef1 src/both.rs\n\
             2 R. N... 100644 100644 100644 abcdef0 abcdef1 R100 src/new.rs\tsrc/old.rs\n\
             u UU N... 100644 100644 100644 100644 abcdef0 abcdef1 abcdef2 src/conflict.rs\n\
             ? src/untracked.rs\n\
             ? src/another.rs\n",
        )
        .unwrap();

        assert_eq!(status.branch, "main");
        assert!(status.dirty);
        assert_eq!(status.untracked, 2);
        assert_eq!(status.modified, 2);
        assert_eq!(status.staged, 3);
        assert_eq!(status.conflicted, 1);
        assert_eq!(status.ahead, 2);
        assert_eq!(status.behind, 1);
        assert_eq!(status.stashed, 3);
    }

    #[test]
    fn detached_git_status_uses_the_short_object_id() {
        let status = parse_git_status(
            "# branch.oid 1cf82045403b6911084598a4487b373dc341638e\n\
             # branch.head (detached)\n",
        )
        .unwrap();

        assert_eq!(status.branch, "@1cf8204");
        assert!(!status.dirty);
        assert_eq!(status.untracked, 0);
        assert_eq!(status.modified, 0);
        assert_eq!(status.staged, 0);
        assert_eq!(status.conflicted, 0);
        assert_eq!(status.ahead, 0);
        assert_eq!(status.behind, 0);
        assert_eq!(status.stashed, 0);
    }

    #[test]
    fn git_summary_reads_clean_and_dirty_repository_state() {
        let repository = TempDir::new().unwrap();
        let initialized = ProcessCommand::new("git")
            .args(["init", "--quiet", "--initial-branch", "main"])
            .arg(repository.path())
            .status()
            .unwrap();
        assert!(initialized.success());

        let clean = git_summary(repository.path()).unwrap();
        assert_eq!(clean.branch, "main");
        assert!(!clean.dirty);
        assert_eq!(clean.untracked, 0);

        fs::write(repository.path().join("README.md"), "new\n").unwrap();
        let dirty = git_summary(repository.path()).unwrap();
        assert_eq!(dirty.branch, "main");
        assert!(dirty.dirty);
        assert_eq!(dirty.untracked, 1);

        let staged = ProcessCommand::new("git")
            .arg("-C")
            .arg(repository.path())
            .args(["add", "README.md"])
            .status()
            .unwrap();
        assert!(staged.success());
        fs::write(repository.path().join("README.md"), "changed again\n").unwrap();
        let both = git_summary(repository.path()).unwrap();
        assert_eq!(both.staged, 1);
        assert_eq!(both.modified, 1);
        assert_eq!(both.untracked, 0);
    }

    #[test]
    fn git_summary_worker_reports_the_matching_project_snapshot() {
        let repository = TempDir::new().unwrap();
        let initialized = ProcessCommand::new("git")
            .args(["init", "--quiet", "--initial-branch", "main"])
            .arg(repository.path())
            .status()
            .unwrap();
        assert!(initialized.success());
        fs::write(repository.path().join("README.md"), "new\n").unwrap();

        let receiver = spawn_git_summary("api".into(), repository.path().to_path_buf());
        let (project_id, summary) = receiver.recv_timeout(Duration::from_secs(2)).unwrap();

        assert_eq!(project_id, "api");
        assert_eq!(summary.branch, "main");
        assert!(summary.dirty);
    }

    #[test]
    fn active_context_is_available_before_the_git_snapshot() {
        let repository = TempDir::new().unwrap();
        let initialized = ProcessCommand::new("git")
            .args(["init", "--quiet", "--initial-branch", "main"])
            .arg(repository.path())
            .status()
            .unwrap();
        assert!(initialized.success());
        let projects = vec![Project {
            id: "api".into(),
            path: repository.path().to_path_buf(),
            group: "Repos".into(),
            name: "api".into(),
            display_name: "API".into(),
        }];
        let file = repository.path().join("src/main.rs");

        let context =
            build_active_project_context(&projects, None, Some(repository.path()), Some(&file))
                .unwrap();

        assert_eq!(context.project_id, "api");
        assert_eq!(context.file.as_deref(), Some("src/main.rs"));
        assert_eq!(context.git, None);
    }

    #[test]
    fn session_poll_interval_starts_after_a_slow_snapshot_finishes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server_url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for _ in 0..5 {
                let (mut stream, _) = listener.accept().unwrap();
                let target = read_target(&mut stream);
                let body = if target.starts_with("/global/health") {
                    r#"{"healthy":true,"version":"1.18.15"}"#
                } else if target.starts_with("/session/status") {
                    "{}"
                } else if target.starts_with("/session?") {
                    thread::sleep(Duration::from_millis(1_100));
                    "[]"
                } else if target.starts_with("/permission") || target.starts_with("/question") {
                    "[]"
                } else {
                    panic!("unexpected OpenCode request {target}");
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let temporary = TempDir::new().unwrap();
        let config = Config::parse("version = 4", temporary.path()).unwrap();
        let cache =
            CacheStore::open(temporary.path().join("cache.json"), config.fingerprint()).unwrap();
        let mut catalog = Catalog::new(config, StdFileSystem, cache);
        let client = opencode::OpenCodeClient::with_registry_dir(
            OpenCodeConfig {
                server_url,
                command: vec!["opencode".into()],
                session_limit: 100,
            },
            temporary.path().join("registry"),
        );
        let unavailable = TcpListener::bind("127.0.0.1:0").unwrap();
        let watcher_url = format!("http://{}", unavailable.local_addr().unwrap());
        drop(unavailable);
        let watcher_client = opencode::OpenCodeClient::with_registry_dir(
            OpenCodeConfig {
                server_url: watcher_url,
                command: vec!["opencode".into()],
                session_limit: 100,
            },
            temporary.path().join("watcher-registry"),
        );
        let mut data = CatalogDataSource {
            catalog: &mut catalog,
            active_project_git: None,
            opencode: Some(OpenCodeDataSource {
                client,
                watcher: watcher_client.watch_shared(),
                last_poll: Instant::now() - Duration::from_secs(2),
            }),
        };

        DataSource::sessions(&mut data, Path::new("/repos/wisp")).unwrap();

        assert!(
            data.opencode.as_ref().unwrap().last_poll.elapsed() < Duration::from_millis(500),
            "poll interval should start when the snapshot completes"
        );
        server.join().unwrap();
    }

    fn read_target(stream: &mut std::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        String::from_utf8(request)
            .unwrap()
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap()
            .to_owned()
    }
}
