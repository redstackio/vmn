//! Core implementation for the `vmn` command-line application.
//!
//! `vmn` is a local Python virtual environment manager. It stores environment
//! metadata in SQLite, discovers existing venvs, creates managed or project-local
//! venvs, and prints stable machine-readable paths used by shell integrations.
//!
//! The public API is intentionally small because `vmn` is primarily a binary
//! crate. The supported interface for users is the CLI installed as `vmn`.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use clap::{Args, Parser, Subcommand, ValueEnum};
use directories::BaseDirs;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use ulid::Ulid;
use walkdir::WalkDir;

const MIGRATION_VERSION: i64 = 1;

#[derive(Parser, Debug)]
#[command(name = "vmn")]
#[command(about = "Rust Python virtual environment manager")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init(InitArgs),
    List(ListArgs),
    Activate(SelectorArgs),
    Deactivate,
    #[command(name = "activate-path")]
    ActivatePath(SelectorArgs),
    Path(SelectorArgs),
    #[command(name = "project-dir")]
    ProjectDir(SelectorArgs),
    Create(CreateArgs),
    Scan(ScanArgs),
    Info(InfoArgs),
    Refresh(RefreshArgs),
    Remove(RemoveArgs),
    Prune(PruneArgs),
    Pythons(PythonsArgs),
    Doctor,
}

#[derive(Args, Debug)]
struct InitArgs {
    #[arg(long, value_enum, default_value_t = Shell::Zsh)]
    shell: Shell,
    #[arg(long)]
    print_shell: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Shell {
    Zsh,
    Bash,
}

#[derive(Args, Debug)]
struct ListArgs {
    #[arg(long)]
    fzf: bool,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    include_missing: bool,
    #[arg(long)]
    all: bool,
}

#[derive(Args, Debug)]
struct SelectorArgs {
    selector: String,
}

#[derive(Args, Debug)]
struct CreateArgs {
    name: String,
    #[arg(long)]
    here: bool,
    #[arg(long)]
    path: Option<PathBuf>,
    #[arg(long, default_value = "python3")]
    python: String,
    #[arg(long)]
    no_activate_output: bool,
}

#[derive(Args, Debug)]
struct ScanArgs {
    #[arg(required = true)]
    dirs: Vec<PathBuf>,
    #[arg(long)]
    max_depth: Option<usize>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct InfoArgs {
    selector: String,
    #[arg(long)]
    packages: bool,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    live: bool,
}

#[derive(Args, Debug)]
struct RefreshArgs {
    selector: Option<String>,
    #[arg(long)]
    all: bool,
}

#[derive(Args, Debug)]
struct RemoveArgs {
    selector: String,
    #[arg(long)]
    delete_files: bool,
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
struct PruneArgs {
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    missing: bool,
    #[arg(long)]
    deleted: bool,
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
struct PythonsArgs {
    #[arg(long)]
    json: bool,
}

pub fn run() -> Result<()> {
    run_with_args(std::env::args_os())
}

pub fn run_with_args<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    let paths = AppPaths::from_env()?;
    let app = App::open(paths)?;
    app.run(cli)
}

struct App {
    paths: AppPaths,
    registry: Registry,
}

impl App {
    fn open(paths: AppPaths) -> Result<Self> {
        paths.ensure_dirs()?;
        let registry = Registry::open(&paths.db_path)?;
        Ok(Self { paths, registry })
    }

    fn run(&self, cli: Cli) -> Result<()> {
        match cli.command {
            Commands::Init(args) => self.init(args),
            Commands::List(args) => self.list(args),
            Commands::Activate(args) => self.activate(&args.selector),
            Commands::Deactivate => self.deactivate_guidance(),
            Commands::ActivatePath(args) => self.activate_path(&args.selector),
            Commands::Path(args) => self.path(&args.selector),
            Commands::ProjectDir(args) => self.project_dir(&args.selector),
            Commands::Create(args) => self.create(args),
            Commands::Scan(args) => self.scan(args),
            Commands::Info(args) => self.info(args),
            Commands::Refresh(args) => self.refresh(args),
            Commands::Remove(args) => self.remove(args),
            Commands::Prune(args) => self.prune(args),
            Commands::Pythons(args) => self.pythons(args),
            Commands::Doctor => self.doctor(),
        }
    }

    fn init(&self, args: InitArgs) -> Result<()> {
        self.paths.ensure_dirs()?;
        if !self.paths.config_path.exists() {
            fs::write(
                &self.paths.config_path,
                "# vmn configuration\n# Generated by vmn init.\n",
            )
            .with_context(|| {
                format!(
                    "failed to write config file {}",
                    self.paths.config_path.display()
                )
            })?;
        }

        match args.shell {
            Shell::Zsh => {
                let _ = args.print_shell;
                println!("{}", zsh_snippet());
            }
            Shell::Bash => {
                let _ = args.print_shell;
                println!("{}", bash_snippet());
            }
        }
        Ok(())
    }

    fn list(&self, args: ListArgs) -> Result<()> {
        let statuses = if args.all {
            StatusFilter::All
        } else if args.include_missing {
            StatusFilter::ActiveAndMissing
        } else {
            StatusFilter::ActiveOnly
        };
        let envs = self.registry.list(statuses)?;

        if args.json {
            print_json(&envs)?;
        } else if args.fzf {
            for env in envs {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    clean_field(&env.id),
                    clean_field(&env.name),
                    clean_field(&env.status),
                    clean_field(&env.path),
                    clean_field(env.python_version.as_deref().unwrap_or("")),
                    clean_field(env.last_used_at.as_deref().unwrap_or(""))
                );
            }
        } else {
            for env in envs {
                let python = env.python_version.as_deref().unwrap_or("-");
                let last_used = env.last_used_at.as_deref().unwrap_or("-");
                println!(
                    "{}  {:<24} {:<8} {:<12} {}",
                    short_id(&env.id),
                    env.name,
                    env.status,
                    python,
                    env.path
                );
                println!("    last used: {last_used}");
            }
        }
        Ok(())
    }

    fn activate(&self, selector: &str) -> Result<()> {
        let env = self.registry.resolve_selector(selector, false)?;
        if env.status != EnvStatus::Active.as_str() {
            bail!("environment '{}' is {}", env.name, env.status);
        }

        let root = PathBuf::from(&env.path);
        if !root.exists() {
            self.registry.mark_status(&env.id, EnvStatus::Missing)?;
            bail!("environment path is missing: {}", root.display());
        }

        let activate = activation_script(&root);
        if !activate.exists() {
            self.registry.mark_status(&env.id, EnvStatus::Missing)?;
            bail!("activation script is missing: {}", activate.display());
        }

        self.registry.update_last_used(&env.id)?;
        launch_activated_subshell(&env, &activate)?;
        Ok(())
    }

    fn deactivate_guidance(&self) -> Result<()> {
        eprintln!(
            "vmn deactivate must run through shell integration to affect your current shell.\n\
             Run `vmn init --shell zsh` or `vmn init --shell bash`, add the snippet to your shell config, then use `vmn deactivate`.\n\
             Without shell integration, `vmn activate <selector>` starts an activated subshell; type `exit` to leave it."
        );
        Ok(())
    }

    fn activate_path(&self, selector: &str) -> Result<()> {
        let env = self.registry.resolve_selector(selector, false)?;
        if env.status != EnvStatus::Active.as_str() {
            bail!("environment '{}' is {}", env.name, env.status);
        }

        let root = PathBuf::from(&env.path);
        if !root.exists() {
            self.registry.mark_status(&env.id, EnvStatus::Missing)?;
            bail!("environment path is missing: {}", root.display());
        }

        let activate = activation_script(&root);
        if !activate.exists() {
            self.registry.mark_status(&env.id, EnvStatus::Missing)?;
            bail!("activation script is missing: {}", activate.display());
        }

        self.registry.update_last_used(&env.id)?;
        println!("{}", activate.display());
        Ok(())
    }

    fn path(&self, selector: &str) -> Result<()> {
        let env = self.registry.resolve_selector(selector, false)?;
        println!("{}", env.path);
        Ok(())
    }

    fn project_dir(&self, selector: &str) -> Result<()> {
        let env = self.registry.resolve_selector(selector, false)?;
        if let Some(project_dir) = env.project_dir {
            println!("{project_dir}");
        }
        Ok(())
    }

    fn create(&self, args: CreateArgs) -> Result<()> {
        if args.here && args.path.is_some() {
            bail!("--here and --path cannot be used together");
        }

        let target = if args.here {
            std::env::current_dir()
                .context("failed to read current directory")?
                .join(".venv")
        } else if let Some(path) = args.path {
            path
        } else {
            validate_managed_name(&args.name)?;
            self.paths.managed_envs_dir.join(&args.name)
        };
        let target = absolute_for_create(&target)?;

        if target.exists() {
            if !is_venv_root(&target) {
                bail!(
                    "target path exists but is not a Python venv: {}",
                    target.display()
                );
            }
        } else {
            fs::create_dir_all(
                target
                    .parent()
                    .ok_or_else(|| anyhow!("target path has no parent: {}", target.display()))?,
            )?;
            let python = resolve_python_selector(&args.python)?;
            run_checked(
                Command::new(&python).arg("-m").arg("venv").arg(&target),
                "failed to create virtual environment",
            )?;
        }

        let now = now();
        let probe = probe_basic(&target);
        let project_dir = if args.here {
            target.parent().map(path_to_string)
        } else {
            None
        };
        let record = NewEnvironment {
            id: Ulid::new().to_string(),
            name: args.name,
            path: path_to_string(&target),
            project_dir,
            source: if target.starts_with(&self.paths.managed_envs_dir) {
                EnvSource::Managed
            } else {
                EnvSource::Manual
            },
            status: EnvStatus::Active,
            python_version: probe.python_version,
            pip_version: probe.pip_version,
            created_at: now.clone(),
            updated_at: now,
            last_used_at: None,
            last_scanned_at: None,
        };

        let env = self.registry.upsert_environment(record)?;
        if !args.no_activate_output {
            println!("{}", activation_script(Path::new(&env.path)).display());
        }
        Ok(())
    }

    fn scan(&self, args: ScanArgs) -> Result<()> {
        let mut found = Vec::new();
        let mut permission_errors = 0usize;

        for dir in args.dirs {
            let mut walker = WalkDir::new(&dir).follow_links(false);
            if let Some(max_depth) = args.max_depth {
                walker = walker.max_depth(max_depth);
            }
            let mut iter = walker.into_iter();

            while let Some(entry) = iter.next() {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        if is_permission_error(&error) {
                            permission_errors += 1;
                            continue;
                        }
                        eprintln!("warning: skipped path during scan: {error}");
                        continue;
                    }
                };

                if !entry.file_type().is_dir() {
                    continue;
                }

                let path = entry.path();
                if is_venv_root(path) {
                    found.push(path.to_path_buf());
                    iter.skip_current_dir();
                }
            }
        }

        let mut records = Vec::new();
        if !args.dry_run {
            for path in &found {
                records.push(self.register_scanned(path)?);
            }
        }

        let summary = ScanSummary {
            found: found.len(),
            registered: records.len(),
            permission_errors,
            paths: found.iter().map(|p| path_to_string(p)).collect(),
        };

        if args.json {
            print_json(&summary)?;
        } else if args.dry_run {
            for path in &summary.paths {
                println!("{path}");
            }
            eprintln!(
                "found {} environment(s), permission errors: {}",
                summary.found, summary.permission_errors
            );
        } else {
            println!(
                "registered {} environment(s), permission errors: {}",
                summary.registered, summary.permission_errors
            );
        }
        Ok(())
    }

    fn register_scanned(&self, path: &Path) -> Result<Environment> {
        let path = absolute_existing(path)?;
        let now = now();
        let probe = probe_basic(&path);
        let existing = self.registry.find_by_path(&path_to_string(&path))?;
        let project_dir = match existing.as_ref() {
            Some(env) => env.project_dir.clone(),
            None => path.parent().map(path_to_string),
        };
        let name = existing
            .as_ref()
            .map(|env| env.name.clone())
            .unwrap_or_else(|| scanned_name(&path));
        let source = existing
            .as_ref()
            .and_then(|env| EnvSource::from_str(&env.source))
            .unwrap_or(EnvSource::Scanned);
        let id = existing
            .as_ref()
            .map(|env| env.id.clone())
            .unwrap_or_else(|| Ulid::new().to_string());
        let created_at = existing
            .as_ref()
            .map(|env| env.created_at.clone())
            .unwrap_or_else(|| now.clone());

        self.registry.upsert_environment(NewEnvironment {
            id,
            name,
            path: path_to_string(&path),
            project_dir,
            source,
            status: EnvStatus::Active,
            python_version: probe.python_version,
            pip_version: probe.pip_version,
            created_at,
            updated_at: now.clone(),
            last_used_at: existing.and_then(|env| env.last_used_at),
            last_scanned_at: Some(now),
        })
    }

    fn info(&self, args: InfoArgs) -> Result<()> {
        let mut env = self.registry.resolve_selector(&args.selector, true)?;
        if args.live {
            refresh_one(&self.registry, &env)?;
            env = self.registry.resolve_selector(&env.id, true)?;
        }

        let package_count = self.registry.package_count(&env.id)?;
        let packages = if args.packages {
            self.registry.packages(&env.id)?
        } else {
            Vec::new()
        };

        if args.json {
            let output = InfoOutput {
                environment: env,
                package_count,
                packages,
            };
            print_json(&output)?;
        } else {
            println!("Name: {}", env.name);
            println!("ID: {}", env.id);
            println!("Status: {}", env.status);
            println!("Path: {}", env.path);
            println!("Project: {}", env.project_dir.as_deref().unwrap_or("-"));
            println!("Python: {}", env.python_version.as_deref().unwrap_or("-"));
            println!("Pip: {}", env.pip_version.as_deref().unwrap_or("-"));
            println!("Packages: {package_count}");
            println!("Source: {}", env.source);
            println!("Last used: {}", env.last_used_at.as_deref().unwrap_or("-"));
            if args.packages {
                for package in packages {
                    println!("  {} {}", package.name, package.version);
                }
            }
        }
        Ok(())
    }

    fn refresh(&self, args: RefreshArgs) -> Result<()> {
        if args.all && args.selector.is_some() {
            bail!("pass either --all or a selector, not both");
        }
        if !args.all && args.selector.is_none() {
            bail!("refresh requires a selector or --all");
        }

        let envs = if args.all {
            self.registry.list(StatusFilter::ActiveAndMissing)?
        } else {
            vec![
                self.registry
                    .resolve_selector(args.selector.as_deref().unwrap(), true)?,
            ]
        };

        let mut refreshed = 0usize;
        let mut missing = 0usize;
        for env in envs {
            match refresh_one(&self.registry, &env) {
                Ok(RefreshState::Refreshed) => refreshed += 1,
                Ok(RefreshState::Missing) => missing += 1,
                Err(error) => eprintln!("warning: failed to refresh {}: {error}", env.name),
            }
        }

        println!("refreshed {refreshed} environment(s), missing {missing}");
        Ok(())
    }

    fn remove(&self, args: RemoveArgs) -> Result<()> {
        let env = self.registry.resolve_selector(&args.selector, true)?;
        if args.delete_files {
            let path = PathBuf::from(&env.path);
            if !path.exists() {
                self.registry.mark_status(&env.id, EnvStatus::Missing)?;
                bail!("environment path is missing: {}", path.display());
            }
            ensure_safe_venv_delete(&path)?;
            if !args.yes {
                confirm(&format!(
                    "Delete virtual environment directory {}?",
                    path.display()
                ))?;
            }
            fs::remove_dir_all(&path)
                .with_context(|| format!("failed to delete {}", path.display()))?;
        }

        self.registry.mark_status(&env.id, EnvStatus::Deleted)?;
        println!("removed {}", env.id);
        Ok(())
    }

    fn prune(&self, args: PruneArgs) -> Result<()> {
        let statuses = prune_statuses(args.missing, args.deleted);
        let records = self.registry.list_by_statuses(&statuses)?;

        if args.dry_run {
            for env in records {
                println!("{}\t{}\t{}\t{}", env.id, env.name, env.status, env.path);
            }
            return Ok(());
        }

        if records.is_empty() {
            println!("nothing to prune");
            return Ok(());
        }

        if !args.yes {
            confirm(&format!("Prune {} registry record(s)?", records.len()))?;
        }

        let ids: Vec<_> = records.iter().map(|env| env.id.as_str()).collect();
        self.registry.delete_environments(&ids)?;
        println!("pruned {} registry record(s)", ids.len());
        Ok(())
    }

    fn pythons(&self, args: PythonsArgs) -> Result<()> {
        let interpreters = discover_python_interpreters();
        if args.json {
            print_json(&interpreters)?;
        } else if interpreters.is_empty() {
            println!("no Python interpreters found on PATH");
        } else {
            for interpreter in interpreters {
                println!(
                    "{:<10} {:<14} {}",
                    interpreter.version_key.as_deref().unwrap_or("-"),
                    interpreter.version.as_deref().unwrap_or("-"),
                    interpreter.path
                );
            }
        }
        Ok(())
    }

    fn doctor(&self) -> Result<()> {
        let mut issues = 0usize;
        report_check(
            "config dir writable",
            dir_writable(&self.paths.config_dir),
            &mut issues,
        );
        report_check(
            "data dir writable",
            dir_writable(&self.paths.data_dir),
            &mut issues,
        );
        report_check("database exists", self.paths.db_path.exists(), &mut issues);
        report_optional_check("fzf installed", which::which("fzf").is_ok());
        let python_count = discover_python_interpreters().len();
        report_check(
            "python available",
            python_count > 0 || which::which("python3").is_ok(),
            &mut issues,
        );
        println!("check python interpreters discovered: {python_count}");

        let active = self.registry.list(StatusFilter::ActiveOnly)?;
        for env in &active {
            let root = PathBuf::from(&env.path);
            if !root.exists() {
                self.registry.mark_status(&env.id, EnvStatus::Missing)?;
                println!("check active path {}: missing", env.name);
                issues += 1;
                continue;
            }
            let activate = activation_script(&root);
            if !activate.exists() {
                println!("check activation script {}: missing", env.name);
                issues += 1;
            }
        }

        let duplicates = self.registry.duplicate_names()?;
        if duplicates.is_empty() {
            println!("check duplicate names: ok");
        } else {
            issues += 1;
            println!("check duplicate names: found");
            for (name, count) in duplicates {
                println!("  {name}: {count}");
            }
        }

        if issues > 0 {
            bail!("doctor found {issues} issue(s)");
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct AppPaths {
    config_dir: PathBuf,
    data_dir: PathBuf,
    config_path: PathBuf,
    db_path: PathBuf,
    managed_envs_dir: PathBuf,
}

impl AppPaths {
    fn from_env() -> Result<Self> {
        if let (Ok(config_dir), Ok(data_dir)) = (
            std::env::var("VMN_CONFIG_DIR"),
            std::env::var("VMN_DATA_DIR"),
        ) {
            return Ok(Self::from_dirs(config_dir, data_dir));
        }

        let base = BaseDirs::new().ok_or_else(|| anyhow!("failed to locate home directory"))?;
        Ok(Self::from_dirs(
            base.config_dir().join("vmn"),
            base.data_dir().join("vmn"),
        ))
    }

    fn from_dirs<C, D>(config_dir: C, data_dir: D) -> Self
    where
        C: Into<PathBuf>,
        D: Into<PathBuf>,
    {
        let config_dir = config_dir.into();
        let data_dir = data_dir.into();
        Self {
            config_path: config_dir.join("config.toml"),
            db_path: data_dir.join("vmn.db"),
            managed_envs_dir: data_dir.join("envs"),
            config_dir,
            data_dir,
        }
    }

    fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.config_dir)
            .with_context(|| format!("failed to create {}", self.config_dir.display()))?;
        fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("failed to create {}", self.data_dir.display()))?;
        fs::create_dir_all(&self.managed_envs_dir)
            .with_context(|| format!("failed to create {}", self.managed_envs_dir.display()))?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum StatusFilter {
    ActiveOnly,
    ActiveAndMissing,
    All,
}

#[derive(Clone, Copy)]
enum EnvSource {
    Managed,
    Scanned,
    Manual,
}

impl EnvSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::Scanned => "scanned",
            Self::Manual => "manual",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "managed" => Some(Self::Managed),
            "scanned" => Some(Self::Scanned),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum EnvStatus {
    Active,
    Missing,
    Deleted,
}

impl EnvStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Missing => "missing",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Environment {
    id: String,
    name: String,
    path: String,
    project_dir: Option<String>,
    source: String,
    status: String,
    python_version: Option<String>,
    pip_version: Option<String>,
    created_at: String,
    updated_at: String,
    last_used_at: Option<String>,
    last_scanned_at: Option<String>,
}

struct NewEnvironment {
    id: String,
    name: String,
    path: String,
    project_dir: Option<String>,
    source: EnvSource,
    status: EnvStatus,
    python_version: Option<String>,
    pip_version: Option<String>,
    created_at: String,
    updated_at: String,
    last_used_at: Option<String>,
    last_scanned_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct PackageRecord {
    name: String,
    version: String,
    captured_at: String,
}

#[derive(Serialize)]
struct InfoOutput {
    environment: Environment,
    package_count: usize,
    packages: Vec<PackageRecord>,
}

#[derive(Serialize)]
struct ScanSummary {
    found: usize,
    registered: usize,
    permission_errors: usize,
    paths: Vec<String>,
}

#[derive(Default)]
struct Probe {
    python_version: Option<String>,
    pip_version: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct PythonInterpreter {
    path: String,
    executable: String,
    version: Option<String>,
    version_key: Option<String>,
}

struct Registry {
    conn: Connection,
}

impl Registry {
    fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open registry {}", path.display()))?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let registry = Self { conn };
        registry.migrate()?;
        Ok(registry)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
              version INTEGER PRIMARY KEY,
              applied_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS environments (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              path TEXT NOT NULL UNIQUE,
              project_dir TEXT,
              source TEXT NOT NULL CHECK (source IN ('managed', 'scanned', 'manual')),
              status TEXT NOT NULL CHECK (status IN ('active', 'missing', 'deleted')),
              python_version TEXT,
              pip_version TEXT,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              last_used_at TEXT,
              last_scanned_at TEXT
            );

            CREATE TABLE IF NOT EXISTS packages (
              env_id TEXT NOT NULL,
              name TEXT NOT NULL,
              version TEXT NOT NULL,
              captured_at TEXT NOT NULL,
              PRIMARY KEY (env_id, name),
              FOREIGN KEY (env_id) REFERENCES environments(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_environments_name ON environments(name);
            CREATE INDEX IF NOT EXISTS idx_environments_status ON environments(status);
            CREATE INDEX IF NOT EXISTS idx_environments_last_used ON environments(last_used_at);
            "#,
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            params![MIGRATION_VERSION, now()],
        )?;
        Ok(())
    }

    fn upsert_environment(&self, env: NewEnvironment) -> Result<Environment> {
        let path = env.path.clone();
        self.conn.execute(
            r#"
            INSERT INTO environments (
                id, name, path, project_dir, source, status, python_version, pip_version,
                created_at, updated_at, last_used_at, last_scanned_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(path) DO UPDATE SET
                name = excluded.name,
                project_dir = excluded.project_dir,
                source = excluded.source,
                status = excluded.status,
                python_version = excluded.python_version,
                pip_version = excluded.pip_version,
                updated_at = excluded.updated_at,
                last_used_at = COALESCE(excluded.last_used_at, environments.last_used_at),
                last_scanned_at = excluded.last_scanned_at
            "#,
            params![
                env.id,
                env.name,
                env.path,
                env.project_dir,
                env.source.as_str(),
                env.status.as_str(),
                env.python_version,
                env.pip_version,
                env.created_at,
                env.updated_at,
                env.last_used_at,
                env.last_scanned_at,
            ],
        )?;
        self.find_by_path(&path)?
            .ok_or_else(|| anyhow!("failed to read updated environment"))
    }

    fn find_by_path(&self, path: &str) -> Result<Option<Environment>> {
        self.conn
            .query_row(
                "SELECT * FROM environments WHERE path = ?1",
                params![path],
                row_to_environment,
            )
            .optional()
            .context("failed to query environment by path")
    }

    fn list(&self, status_filter: StatusFilter) -> Result<Vec<Environment>> {
        let sql = match status_filter {
            StatusFilter::ActiveOnly => {
                "SELECT * FROM environments WHERE status = 'active' ORDER BY last_used_at IS NULL, last_used_at DESC, lower(name), path"
            }
            StatusFilter::ActiveAndMissing => {
                "SELECT * FROM environments WHERE status IN ('active', 'missing') ORDER BY last_used_at IS NULL, last_used_at DESC, lower(name), path"
            }
            StatusFilter::All => {
                "SELECT * FROM environments ORDER BY last_used_at IS NULL, last_used_at DESC, lower(name), path"
            }
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([], row_to_environment)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to list environments")
    }

    fn list_by_statuses(&self, statuses: &[EnvStatus]) -> Result<Vec<Environment>> {
        let all = self.list(StatusFilter::All)?;
        let wanted: HashSet<_> = statuses.iter().map(|status| status.as_str()).collect();
        Ok(all
            .into_iter()
            .filter(|env| wanted.contains(env.status.as_str()))
            .collect())
    }

    fn resolve_selector(&self, selector: &str, include_deleted: bool) -> Result<Environment> {
        let mut matches = BTreeMap::new();
        for env in self.list(StatusFilter::All)? {
            if !include_deleted && env.status == EnvStatus::Deleted.as_str() {
                continue;
            }
            if env.id == selector || env.id.starts_with(selector) || env.name == selector {
                matches.insert(env.id.clone(), env);
            }
        }

        match matches.len() {
            0 => bail!("no environment matches selector '{selector}'"),
            1 => Ok(matches.into_values().next().unwrap()),
            _ => {
                let mut message = format!("selector '{selector}' is ambiguous:");
                for env in matches.values() {
                    message.push_str(&format!("\n  {}  {}  {}", env.id, env.name, env.path));
                }
                bail!(message);
            }
        }
    }

    fn mark_status(&self, id: &str, status: EnvStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE environments SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.as_str(), now(), id],
        )?;
        Ok(())
    }

    fn update_last_used(&self, id: &str) -> Result<()> {
        let timestamp = now();
        self.conn.execute(
            "UPDATE environments SET last_used_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![timestamp, id],
        )?;
        Ok(())
    }

    fn update_probe(&self, id: &str, probe: &Probe) -> Result<()> {
        self.conn.execute(
            r#"
            UPDATE environments
            SET python_version = ?1, pip_version = ?2, status = 'active', updated_at = ?3
            WHERE id = ?4
            "#,
            params![probe.python_version, probe.pip_version, now(), id],
        )?;
        Ok(())
    }

    fn replace_packages(&self, id: &str, packages: &[PipPackage]) -> Result<()> {
        let captured_at = now();
        self.with_immediate_transaction(|| {
            self.conn
                .execute("DELETE FROM packages WHERE env_id = ?1", params![id])?;
            let mut stmt = self.conn.prepare(
                "INSERT INTO packages (env_id, name, version, captured_at) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for package in packages {
                stmt.execute(params![id, package.name, package.version, captured_at])?;
            }
            Ok(())
        })
    }

    fn package_count(&self, id: &str) -> Result<usize> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM packages WHERE env_id = ?1",
                params![id],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as usize)
            .context("failed to count packages")
    }

    fn packages(&self, id: &str) -> Result<Vec<PackageRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, version, captured_at FROM packages WHERE env_id = ?1 ORDER BY lower(name)",
        )?;
        let rows = stmt.query_map(params![id], |row| {
            Ok(PackageRecord {
                name: row.get(0)?,
                version: row.get(1)?,
                captured_at: row.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to list packages")
    }

    fn delete_environments(&self, ids: &[&str]) -> Result<()> {
        self.with_immediate_transaction(|| {
            for id in ids {
                self.conn
                    .execute("DELETE FROM environments WHERE id = ?1", params![id])?;
            }
            Ok(())
        })
    }

    fn duplicate_names(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, COUNT(*) FROM environments WHERE status != 'deleted' GROUP BY name HAVING COUNT(*) > 1 ORDER BY lower(name)",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to find duplicate names")
    }

    fn with_immediate_transaction<F>(&self, operation: F) -> Result<()>
    where
        F: FnOnce() -> Result<()>,
    {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        match operation() {
            Ok(()) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }
}

fn row_to_environment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Environment> {
    Ok(Environment {
        id: row.get("id")?,
        name: row.get("name")?,
        path: row.get("path")?,
        project_dir: row.get("project_dir")?,
        source: row.get("source")?,
        status: row.get("status")?,
        python_version: row.get("python_version")?,
        pip_version: row.get("pip_version")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        last_used_at: row.get("last_used_at")?,
        last_scanned_at: row.get("last_scanned_at")?,
    })
}

fn zsh_snippet() -> &'static str {
    r##"# VMN zsh integration
_vmn_activate() {
  local selector project_dir activate_path
  selector="$1"
  [[ -z "$selector" ]] && return 1

  project_dir=$(command vmn project-dir "$selector") || return
  if [[ -n "$project_dir" ]]; then
    cd "$project_dir" || return
  fi

  activate_path=$(command vmn activate-path "$selector") || return
  source "$activate_path"
}

_vmn_deactivate() {
  if (( $+functions[deactivate] )); then
    deactivate
  else
    echo "No active Python virtual environment." >&2
    return 1
  fi
}

vmn() {
  case "$1" in
    activate)
      shift
      if [[ $# -ne 1 ]]; then
        echo "usage: vmn activate <name-or-id>" >&2
        return 2
      fi
      _vmn_activate "$1"
      ;;
    deactivate)
      shift
      if [[ $# -ne 0 ]]; then
        echo "usage: vmn deactivate" >&2
        return 2
      fi
      _vmn_deactivate
      ;;
    *)
      command vmn "$@"
      ;;
  esac
}

v() {
  local selected id

  if ! command -v fzf >/dev/null 2>&1; then
    echo 'vmn: fzf is required for the interactive picker. Install fzf or use "vmn list" and "vmn activate <selector>".' >&2
    return 1
  fi

  selected=$(command vmn list --fzf | fzf --delimiter=$'\t' --with-nth=2,3,4 --height=40% --reverse) || return
  id=${selected%%$'\t'*}
  [[ -z "$id" ]] && return
  _vmn_activate "$id"
}

vd() {
  _vmn_deactivate
}

_vmn_widget() {
  zle -I
  v
  zle reset-prompt
}

_vmn_deactivate_widget() {
  zle -I
  vd
  zle reset-prompt
}

if [[ -o interactive ]]; then
  zle -N _vmn_widget 2>/dev/null
  zle -N _vmn_deactivate_widget 2>/dev/null

  # Ctrl-F opens the VMN picker.
  bindkey '^F' _vmn_widget 2>/dev/null

  # Ctrl-Shift-F is only distinguishable in terminals that emit CSI-u keys.
  # Ctrl-X Ctrl-F is a portable fallback for deactivating the current venv.
  bindkey $'\e[70;6u' _vmn_deactivate_widget 2>/dev/null
  bindkey '^X^F' _vmn_deactivate_widget 2>/dev/null
fi"##
}

fn bash_snippet() -> &'static str {
    r##"# VMN bash integration
_vmn_activate() {
  local selector project_dir activate_path
  selector="$1"
  [[ -z "$selector" ]] && return 1

  project_dir=$(command vmn project-dir "$selector") || return
  if [[ -n "$project_dir" ]]; then
    cd "$project_dir" || return
  fi

  activate_path=$(command vmn activate-path "$selector") || return
  source "$activate_path"
}

_vmn_deactivate() {
  if declare -F deactivate >/dev/null; then
    deactivate
  else
    echo "No active Python virtual environment." >&2
    return 1
  fi
}

vmn() {
  case "$1" in
    activate)
      shift
      if [[ $# -ne 1 ]]; then
        echo "usage: vmn activate <name-or-id>" >&2
        return 2
      fi
      _vmn_activate "$1"
      ;;
    deactivate)
      shift
      if [[ $# -ne 0 ]]; then
        echo "usage: vmn deactivate" >&2
        return 2
      fi
      _vmn_deactivate
      ;;
    *)
      command vmn "$@"
      ;;
  esac
}

v() {
  local selected id

  if ! command -v fzf >/dev/null 2>&1; then
    echo 'vmn: fzf is required for the interactive picker. Install fzf or use "vmn list" and "vmn activate <selector>".' >&2
    return 1
  fi

  selected=$(command vmn list --fzf | fzf --delimiter=$'\t' --with-nth=2,3,4 --height=40% --reverse) || return
  id=${selected%%$'\t'*}
  [[ -z "$id" ]] && return
  _vmn_activate "$id"
}

vd() {
  _vmn_deactivate
}

if [[ $- == *i* ]]; then
  # Ctrl-F opens the VMN picker.
  bind -x '"\C-f": v' 2>/dev/null

  # Ctrl-Shift-F is only distinguishable in terminals that emit CSI-u keys.
  # Ctrl-X Ctrl-F is a portable fallback for deactivating the current venv.
  bind -x '"\e[70;6u": vd' 2>/dev/null
  bind -x '"\C-x\C-f": vd' 2>/dev/null
fi"##
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn clean_field(value: &str) -> String {
    value.replace(['\t', '\n', '\r'], " ")
}

fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn absolute_existing(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("failed to resolve {}", path.display()))
}

fn absolute_for_create(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        absolute_existing(path)
    } else if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn validate_managed_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        bail!("environment name cannot be empty");
    }
    if name.contains(std::path::MAIN_SEPARATOR) || name.contains('/') || name.contains('\\') {
        bail!("managed environment name cannot contain path separators");
    }
    Ok(())
}

fn activation_script(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("Scripts").join("activate")
    } else {
        root.join("bin").join("activate")
    }
}

fn python_executable(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("Scripts").join("python.exe")
    } else {
        root.join("bin").join("python")
    }
}

fn is_venv_root(path: &Path) -> bool {
    path.join("pyvenv.cfg").is_file() && activation_script(path).is_file()
}

fn launch_activated_subshell(env: &Environment, activate: &Path) -> Result<()> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "/bin/sh".to_string());
    let project_dir = env.project_dir.clone().unwrap_or_default();

    let status = Command::new(&shell)
        .arg("-c")
        .arg(
            r#"if [ -n "$1" ]; then
  cd "$1" || exit 101
fi
. "$2" || exit 102
if [ -n "${VMN_ACTIVATE_TEST_NO_EXEC:-}" ]; then
  printf '%s\n' "${VIRTUAL_ENV:-}"
  pwd
  exit 0
fi
printf 'vmn: activated %s. Type exit to return to the previous shell.\n' "$3" >&2
exec "${SHELL:-/bin/sh}" -i
exit 103"#,
        )
        .arg("vmn-activate")
        .arg(&project_dir)
        .arg(activate)
        .arg(&env.name)
        .env("SHELL", &shell)
        .status()
        .with_context(|| format!("failed to launch shell {shell}"))?;

    match status.code() {
        Some(101) => bail!("failed to change to project directory {project_dir}"),
        Some(102) => bail!("failed to activate {}", activate.display()),
        Some(103) => bail!("failed to start interactive shell {shell}"),
        _ => Ok(()),
    }
}

fn is_permission_error(error: &walkdir::Error) -> bool {
    error
        .io_error()
        .is_some_and(|err| err.kind() == io::ErrorKind::PermissionDenied)
}

fn run_checked(command: &mut Command, context: &str) -> Result<()> {
    let output = command.output().with_context(|| context.to_string())?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    bail!("{context}: {stderr}{stdout}");
}

fn command_stdout(command: &mut Command) -> Result<String> {
    let output = command.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{stderr}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().to_string())
}

fn command_output_text(command: &mut Command) -> Result<String> {
    let output = command.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return Ok(stdout);
    }

    Ok(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

fn probe_basic(root: &Path) -> Probe {
    let python = python_executable(root);
    let python_version = command_output_text(Command::new(&python).arg("--version")).ok();
    let pip_version =
        command_stdout(Command::new(&python).arg("-m").arg("pip").arg("--version")).ok();
    Probe {
        python_version,
        pip_version,
    }
}

#[derive(Deserialize)]
struct PipPackage {
    name: String,
    version: String,
}

fn probe_packages(root: &Path) -> Result<Vec<PipPackage>> {
    let python = python_executable(root);
    let output = command_stdout(
        Command::new(&python)
            .arg("-m")
            .arg("pip")
            .arg("list")
            .arg("--format=json"),
    )?;
    serde_json::from_str(&output).context("failed to parse pip package list")
}

fn resolve_python_selector(selector: &str) -> Result<PathBuf> {
    if selector.trim().is_empty() {
        bail!("Python selector cannot be empty");
    }

    let selector_path = Path::new(selector);
    if selector_path.components().count() > 1 || selector_path.is_absolute() {
        if selector_path.exists() {
            return absolute_existing(selector_path);
        }
        bail!("Python executable not found: {selector}");
    }

    if let Ok(path) = which::which(selector) {
        return Ok(path);
    }

    let interpreters = discover_python_interpreters();
    let matches: Vec<_> = interpreters
        .into_iter()
        .filter(|interpreter| python_selector_matches(selector, interpreter))
        .collect();

    match matches.len() {
        0 => bail!(
            "no Python interpreter matches '{selector}'. Run 'vmn pythons' to see installed versions"
        ),
        _ => Ok(PathBuf::from(&matches[0].path)),
    }
}

fn python_selector_matches(selector: &str, interpreter: &PythonInterpreter) -> bool {
    let normalized = selector.strip_prefix("python").unwrap_or(selector);
    interpreter.version_key.as_deref() == Some(selector)
        || interpreter.version_key.as_deref() == Some(normalized)
        || interpreter
            .version
            .as_deref()
            .is_some_and(|version| version == selector || version == normalized)
        || interpreter.executable == selector
}

fn discover_python_interpreters() -> Vec<PythonInterpreter> {
    let Some(path_var) = std::env::var_os("PATH") else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut interpreters = Vec::new();

    for dir in std::env::split_paths(&path_var) {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let Some(name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };

            if !is_python_executable_name(name) {
                continue;
            }

            let canonical = path.canonicalize().unwrap_or(path);
            if !seen.insert(canonical.clone()) {
                continue;
            }

            if let Some(interpreter) = inspect_python_interpreter(&canonical) {
                interpreters.push(interpreter);
            }
        }
    }

    interpreters.sort_by(|left, right| {
        python_version_sort_key(right)
            .cmp(&python_version_sort_key(left))
            .then_with(|| left.path.cmp(&right.path))
    });
    interpreters
}

fn is_python_executable_name(name: &str) -> bool {
    let name = name.strip_suffix(".exe").unwrap_or(name);
    if name == "python" || name == "python3" {
        return true;
    }

    let Some(version) = name.strip_prefix("python") else {
        return false;
    };
    !version.is_empty()
        && version
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
}

fn inspect_python_interpreter(path: &Path) -> Option<PythonInterpreter> {
    let version_output = command_output_text(Command::new(path).arg("--version")).ok()?;
    let version = parse_python_version(&version_output)?;
    let version_key = major_minor_version(&version);
    Some(PythonInterpreter {
        path: path_to_string(path),
        executable: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path_to_string(path)),
        version: Some(version),
        version_key,
    })
}

fn parse_python_version(output: &str) -> Option<String> {
    let version = output.trim().strip_prefix("Python ")?;
    let cleaned: String = version
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn major_minor_version(version: &str) -> Option<String> {
    let mut parts = version.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    Some(format!("{major}.{minor}"))
}

fn python_version_sort_key(interpreter: &PythonInterpreter) -> (u64, u64, u64) {
    let Some(version) = interpreter.version.as_deref() else {
        return (0, 0, 0);
    };
    let mut parts = version
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

enum RefreshState {
    Refreshed,
    Missing,
}

fn refresh_one(registry: &Registry, env: &Environment) -> Result<RefreshState> {
    let root = PathBuf::from(&env.path);
    if !root.exists() || !is_venv_root(&root) {
        registry.mark_status(&env.id, EnvStatus::Missing)?;
        return Ok(RefreshState::Missing);
    }

    let probe = probe_basic(&root);
    registry.update_probe(&env.id, &probe)?;

    match probe_packages(&root) {
        Ok(packages) => registry.replace_packages(&env.id, &packages)?,
        Err(error) => eprintln!(
            "warning: failed to capture packages for {}: {error}",
            env.name
        ),
    }

    Ok(RefreshState::Refreshed)
}

fn scanned_name(path: &Path) -> String {
    if path.file_name() == Some(OsStr::new(".venv")) {
        path.parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".venv".to_string())
    } else {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "venv".to_string())
    }
}

fn ensure_safe_venv_delete(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("path does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("path is not a directory: {}", path.display());
    }
    if !path.join("pyvenv.cfg").is_file() {
        bail!(
            "refusing to delete path without pyvenv.cfg: {}",
            path.display()
        );
    }
    if !activation_script(path).is_file() {
        bail!(
            "refusing to delete path without activation script: {}",
            path.display()
        );
    }
    Ok(())
}

fn confirm(prompt: &str) -> Result<()> {
    eprint!("{prompt} Type 'yes' to continue: ");
    io::stderr().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    if input.trim() == "yes" {
        Ok(())
    } else {
        bail!("aborted")
    }
}

fn prune_statuses(missing: bool, deleted: bool) -> Vec<EnvStatus> {
    let mut statuses = Vec::new();
    if missing {
        statuses.push(EnvStatus::Missing);
    }
    if deleted {
        statuses.push(EnvStatus::Deleted);
    }
    if statuses.is_empty() {
        statuses.push(EnvStatus::Missing);
        statuses.push(EnvStatus::Deleted);
    }
    statuses
}

fn dir_writable(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    let probe = path.join(".vmn-write-test");
    match fs::write(&probe, b"test") {
        Ok(()) => {
            let _ = fs::remove_file(probe);
            true
        }
        Err(_) => false,
    }
}

fn report_check(name: &str, ok: bool, issues: &mut usize) {
    if ok {
        println!("check {name}: ok");
    } else {
        println!("check {name}: failed");
        *issues += 1;
    }
}

fn report_optional_check(name: &str, ok: bool) {
    if ok {
        println!("check {name}: ok");
    } else {
        println!("check {name}: warning (optional)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_app() -> (TempDir, App) {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_dirs(temp.path().join("config"), temp.path().join("data"));
        let app = App::open(paths).unwrap();
        (temp, app)
    }

    fn fake_env(path: &Path) {
        fs::create_dir_all(path.join("bin")).unwrap();
        fs::write(path.join("pyvenv.cfg"), "home = /usr/bin\n").unwrap();
        fs::write(path.join("bin").join("activate"), "# activate\n").unwrap();
    }

    #[test]
    fn registry_resolves_unique_name_and_rejects_ambiguity() {
        let (_temp, app) = test_app();
        let now = now();
        for (id, path) in [("01ABC", "/tmp/a"), ("01DEF", "/tmp/b")] {
            app.registry
                .upsert_environment(NewEnvironment {
                    id: id.to_string(),
                    name: "api".to_string(),
                    path: path.to_string(),
                    project_dir: None,
                    source: EnvSource::Manual,
                    status: EnvStatus::Active,
                    python_version: None,
                    pip_version: None,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    last_used_at: None,
                    last_scanned_at: None,
                })
                .unwrap();
        }

        assert!(app.registry.resolve_selector("01A", false).is_ok());
        let error = app.registry.resolve_selector("api", false).unwrap_err();
        assert!(error.to_string().contains("ambiguous"));
    }

    #[test]
    fn scan_registers_pyvenv_cfg_environment() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let venv = project.join(".venv");
        fake_env(&venv);

        let paths = AppPaths::from_dirs(temp.path().join("config"), temp.path().join("data"));
        let app = App::open(paths).unwrap();
        app.scan(ScanArgs {
            dirs: vec![project],
            max_depth: None,
            dry_run: false,
            json: false,
        })
        .unwrap();

        let envs = app.registry.list(StatusFilter::All).unwrap();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].name, "project");
        assert_eq!(envs[0].source, "scanned");
    }

    #[test]
    fn scan_preserves_existing_registered_name() {
        let (_temp, app) = test_app();
        let project = app.paths.data_dir.join("project-a");
        let venv = project.join(".venv");
        fake_env(&venv);
        let now = now();

        app.registry
            .upsert_environment(NewEnvironment {
                id: "01ABC".to_string(),
                name: "alpha".to_string(),
                path: path_to_string(&venv),
                project_dir: Some(path_to_string(&project)),
                source: EnvSource::Manual,
                status: EnvStatus::Active,
                python_version: None,
                pip_version: None,
                created_at: now.clone(),
                updated_at: now,
                last_used_at: None,
                last_scanned_at: None,
            })
            .unwrap();

        app.scan(ScanArgs {
            dirs: vec![project],
            max_depth: None,
            dry_run: false,
            json: false,
        })
        .unwrap();

        let env = app.registry.resolve_selector("alpha", false).unwrap();
        assert_eq!(env.name, "alpha");
        assert_eq!(env.source, "manual");
    }

    #[test]
    fn scan_preserves_existing_empty_project_dir() {
        let (_temp, app) = test_app();
        let venv = app.paths.managed_envs_dir.join("managed");
        fake_env(&venv);
        let now = now();

        app.registry
            .upsert_environment(NewEnvironment {
                id: "01ABC".to_string(),
                name: "managed".to_string(),
                path: path_to_string(&venv),
                project_dir: None,
                source: EnvSource::Managed,
                status: EnvStatus::Active,
                python_version: None,
                pip_version: None,
                created_at: now.clone(),
                updated_at: now,
                last_used_at: None,
                last_scanned_at: None,
            })
            .unwrap();

        app.scan(ScanArgs {
            dirs: vec![app.paths.data_dir.clone()],
            max_depth: None,
            dry_run: false,
            json: false,
        })
        .unwrap();

        let env = app.registry.resolve_selector("managed", false).unwrap();
        assert_eq!(env.project_dir, None);
        assert_eq!(env.source, "managed");
    }

    #[test]
    fn delete_safeguard_requires_pyvenv_cfg() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("not-a-venv");
        fs::create_dir_all(&path).unwrap();
        assert!(ensure_safe_venv_delete(&path).is_err());
    }
}
