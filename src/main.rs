//! Entry point for the Azure Pipelines CLI dashboard.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;

use azure_pipelines_cli::client::http::AdoClient;
use azure_pipelines_cli::config::{Config, check_azure_cli};
use azure_pipelines_cli::state;
use azure_pipelines_cli::update;

#[derive(Parser)]
#[command(name = "pipelines", about = "TUI dashboard for Azure DevOps Pipelines")]
struct Cli {
    /// Path to the config file.
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Updates to the latest release from GitHub.
    Update,
    /// Prints the current version.
    Version,
}

/// Provides an RAII guard that disables mouse capture and restores the terminal on drop.
/// Terminal setup (raw mode, alternate screen, panic hook) is handled by
/// `ratatui::init()` — this guard only adds mouse capture cleanup.
struct MouseGuard;

impl Drop for MouseGuard {
    fn drop(&mut self) {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
        ratatui::restore();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handles subcommands that don't need the TUI.
    if let Some(Command::Update) = cli.command {
        return run_update().await;
    }
    if let Some(Command::Version) = cli.command {
        println!("pipelines v{}", update::current_version());
        return Ok(());
    }

    // Pre-TUI checks: ensure Azure CLI or Developer CLI is available.
    check_azure_cli()?;

    // Resolves config path and loads early (if it exists) so the log level
    // is available before tracing initializes.
    let (config_path, config_exists) = Config::resolve_path(cli.config.as_ref())?;
    let early_config = if config_exists {
        Some(Config::load(Some(&config_path))?)
    } else {
        None
    };

    // Initializes tracing with file-based output (avoids polluting the TUI).
    // Uses the configured level as default; RUST_LOG env var overrides.
    let log_level = early_config
        .as_ref()
        .map(|c| c.logging.level.as_str())
        .unwrap_or("info");
    let log_dir_override = early_config
        .as_ref()
        .and_then(|c| c.logging.log_directory.as_deref());
    let max_log_files = early_config
        .as_ref()
        .map(|c| c.logging.max_log_files)
        .unwrap_or(5);
    init_tracing(log_level, log_dir_override, max_log_files);

    // ratatui::init() sets up raw mode, alternate screen, and a panic hook.
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;
    let _guard = MouseGuard;

    let mut config = match early_config {
        Some(c) => {
            tracing::info!(
                org = c.azure_devops.organization,
                project = c.azure_devops.project,
                "app starting"
            );
            c
        }
        None => {
            // No config file — run interactive setup inside the TUI.
            let result = azure_pipelines_cli::render::setup::run_setup(&mut terminal, &config_path);

            match result {
                Ok(Some(config)) => {
                    tracing::info!(
                        org = config.azure_devops.organization,
                        project = config.azure_devops.project,
                        "config created via setup"
                    );
                    config
                }
                Ok(None) => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    };

    loop {
        let client = AdoClient::new(
            &config.azure_devops.organization,
            &config.azure_devops.project,
        )
        .await?;

        tracing::info!("api client connected");

        let result = state::run::run(&mut terminal, client, &config, config_path.clone()).await?;

        match result {
            state::run::RunResult::Quit => break,
            state::run::RunResult::Reload => {
                tracing::info!("reloading application");
                config = Config::load(Some(&config_path))?;
            }
        }
    }

    Ok(())
}

async fn run_update() -> Result<()> {
    println!("Current version: v{}", update::current_version());
    println!("Checking for updates...");

    match update::self_update().await {
        Ok(result) => {
            println!("Updated to v{}", result.version);
            println!("Binary installed at: {}", result.path.display());
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.starts_with("Already on latest version") {
                println!("{msg}");
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

/// Initializes tracing to log to a rolling daily file.
/// Uses the given level as default; `RUST_LOG` env var overrides if set.
/// Logs go to `log_dir_override` if set, otherwise `~/.local/state/pipelines/`.
/// Retains up to `max_log_files` daily log files.
fn init_tracing(level: &str, log_dir_override: Option<&str>, max_log_files: usize) {
    use tracing_appender::rolling;
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt;
    use tracing_subscriber::prelude::*;

    let filter = EnvFilter::builder()
        .with_default_directive(
            level
                .parse()
                .unwrap_or_else(|_| tracing::level_filters::LevelFilter::INFO.into()),
        )
        .from_env_lossy()
        .add_directive("hyper_util=warn".parse().unwrap())
        .add_directive("hyper=warn".parse().unwrap())
        .add_directive("reqwest=warn".parse().unwrap())
        .add_directive("mio=warn".parse().unwrap());

    let log_dir = if let Some(dir) = log_dir_override {
        std::path::PathBuf::from(dir)
    } else {
        match dirs::home_dir() {
            Some(h) => h.join(".local/state/pipelines"),
            None => {
                eprintln!("warning: could not determine home directory; file logging disabled");
                return;
            }
        }
    };

    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!(
            "warning: failed to create log directory {}: {e}; file logging disabled",
            log_dir.display()
        );
        return;
    }

    let file_appender = rolling::RollingFileAppender::builder()
        .rotation(rolling::Rotation::DAILY)
        .filename_prefix("pipelines.log")
        .max_log_files(max_log_files)
        .build(&log_dir);

    let file_appender = match file_appender {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "warning: failed to create log appender in {}: {e}; file logging disabled",
                log_dir.display()
            );
            return;
        }
    };

    let file_layer = fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false)
        .with_target(true);

    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .init();

    tracing::info!(
        log_dir = %log_dir.display(),
        max_files = max_log_files,
        level,
        "tracing initialized"
    );
}
