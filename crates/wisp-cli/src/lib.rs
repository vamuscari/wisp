use std::{
    env,
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::{SystemTime, UNIX_EPOCH},
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
    protocol::{HostContext, PROTOCOL_VERSION, Selection, SelectionEnvelope, SelectionStatus},
};
use wisp_tui::{App, DataSource, TuiError};

#[derive(Debug, Parser)]
#[command(
    name = "wisp",
    version,
    about = "Pick projects and files from the terminal"
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
    Open {
        #[arg(value_name = "SELECTION_JSON")]
        selection_json: String,
    },
}

#[derive(Clone, Debug, Default, Args)]
struct PickArgs {
    #[arg(long, value_name = "PATH")]
    result_file: Option<PathBuf>,
    #[arg(long, value_name = "PATH")]
    host_context_file: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = InitialView::Projects)]
    initial_view: InitialView,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum InitialView {
    #[default]
    Projects,
    Windows,
}

#[derive(Clone, Copy, Debug, Subcommand)]
enum CacheCommand {
    Clear,
}

#[derive(Clone, Copy, Debug, Subcommand)]
enum ConfigCommand {
    Validate,
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
    #[error("unsupported selection protocol version {0}")]
    UnsupportedProtocol(u32),
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
        args.initial_view,
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
                serde_json::to_writer_pretty(&mut writer, &projects)?;
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
        WispCommand::Open { selection_json } => open_selection(&selection_json),
        WispCommand::Pick(_) => unreachable!("pick is handled before noninteractive commands"),
    }
}

fn pick(
    config_override: Option<&Path>,
    host_context_path: Option<&Path>,
    initial_view: InitialView,
) -> Result<Option<Selection>, CliError> {
    let (config, mut catalog) = load_catalog(config_override)?;
    let projects = catalog.projects(now())?;
    let context = host_context_path.map(read_host_context).transpose()?;
    let app = App::new(
        projects,
        config.openers,
        config.follow_symlinks,
        context,
        match initial_view {
            InitialView::Projects => wisp_tui::InitialView::Projects,
            InitialView::Windows => wisp_tui::InitialView::Windows,
        },
    );
    let mut data = CatalogDataSource {
        catalog: &mut catalog,
    };
    Ok(wisp_tui::run(app, &mut data)?)
}

struct CatalogDataSource<'a> {
    catalog: &'a mut Catalog<StdFileSystem>,
}

impl DataSource for CatalogDataSource<'_> {
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
    if envelope.protocol_version != PROTOCOL_VERSION {
        return Err(CliError::UnsupportedProtocol(envelope.protocol_version));
    }
    if envelope.status != SelectionStatus::Selected {
        return Err(CliError::NotSelected);
    }
    let selection = envelope.selection.ok_or(CliError::NotSelected)?;
    let opener = match selection {
        Selection::Project { opener, .. } | Selection::File { opener, .. } => {
            opener.ok_or(CliError::MissingOpener)?
        }
        Selection::CloseProject { .. } | Selection::HostItem { .. } => {
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
