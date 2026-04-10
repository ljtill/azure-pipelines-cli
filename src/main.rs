use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;

use azure_pipelines_cli::api::client::AdoClient;
use azure_pipelines_cli::app;
use azure_pipelines_cli::config::{Config, check_azure_cli};
use azure_pipelines_cli::update;

#[derive(Parser)]
#[command(name = "pipelines", about = "TUI dashboard for Azure DevOps Pipelines")]
struct Cli {
    /// Path to config file
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Update to the latest release from GitHub
    Update,
}

/// RAII guard that disables mouse capture and restores the terminal on drop.
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

    // Handle subcommands that don't need the TUI
    if let Some(Command::Update) = cli.command {
        return run_update().await;
    }

    // Pre-TUI checks: ensure Azure CLI or Developer CLI is available.
    check_azure_cli()?;

    // Resolve config path and load early (if it exists) so the log level
    // is available before tracing initializes.
    let (config_path, config_exists) = Config::resolve_path(cli.config.as_ref())?;
    let early_config = if config_exists {
        Some(Config::load(Some(&config_path))?)
    } else {
        None
    };

    // Initialize tracing with file-based output (avoids polluting the TUI).
    // Uses the configured level as default; RUST_LOG env var overrides.
    let log_level = early_config
        .as_ref()
        .map(|c| c.logging.level.as_str())
        .unwrap_or("info");
    init_tracing(log_level);

    // ratatui::init() sets up raw mode, alternate screen, and a panic hook.
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;
    let _guard = MouseGuard;

    let config = match early_config {
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
            let result = azure_pipelines_cli::ui::setup::run_setup(&mut terminal, &config_path);

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

    let client = AdoClient::new(
        &config.azure_devops.organization,
        &config.azure_devops.project,
    )
    .await?;

    tracing::info!("api client connected");

    app::run::run(&mut terminal, client, &config, config_path).await
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

/// Initialize tracing to log to a file.
/// Uses the given level as default; `RUST_LOG` env var overrides if set.
/// Logs go to `~/.local/state/pipelines/debug.log`.
fn init_tracing(level: &str) {
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

    let log_dir = match dirs::home_dir() {
        Some(h) => h.join(".local/state/pipelines"),
        None => return, // No home directory — skip file logging
    };
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("debug.log");

    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
    else {
        return;
    };

    let file_layer = fmt::layer()
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false)
        .with_target(true);

    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .init();

    tracing::info!("tracing initialized, logging to {}", log_path.display());
}
