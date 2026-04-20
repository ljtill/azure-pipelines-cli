//! Entry point for the Azure DevOps CLI dashboard.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;

use azure_devops_cli::client::endpoints::DEFAULT_API_VERSION;
use azure_devops_cli::client::http::AdoClient;
use azure_devops_cli::config::{Config, check_azure_cli};
use azure_devops_cli::state;
use azure_devops_cli::state::run::LogInitStatus;
use azure_devops_cli::update;

#[derive(Parser)]
#[command(name = "devops", about = "TUI dashboard for Azure DevOps")]
struct Cli {
    /// Path to the config file.
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Azure DevOps REST API version to use. Defaults to 7.1.
    #[arg(
        long,
        global = true,
        env = "DEVOPS_API_VERSION",
        default_value = DEFAULT_API_VERSION,
        value_name = "VERSION",
        help = "Azure DevOps REST API version to use. Defaults to 7.1."
    )]
    api_version: String,

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
        println!("devops v{}", update::current_version());
        return Ok(());
    }

    // Pre-TUI checks: ensure Azure CLI or Developer CLI is available.
    check_azure_cli()?;

    // Resolves config path and loads early (if it exists) so the log level
    // is available before tracing initializes.
    let (config_path, config_exists) = Config::resolve_path(cli.config.as_ref()).await?;
    let early_config = if config_exists {
        Some(Config::load(Some(&config_path)).await?)
    } else {
        None
    };

    // Initializes tracing with file-based output (avoids polluting the TUI).
    // Uses the configured level as default; RUST_LOG env var overrides.
    let log_level = early_config
        .as_ref()
        .map_or("info", |c| c.logging.level.as_str());
    let log_dir_override = early_config
        .as_ref()
        .and_then(|c| c.logging.log_directory.as_deref());
    let max_log_files = early_config.as_ref().map_or(5, |c| c.logging.max_log_files);
    let log_init_status = init_tracing(log_level, log_dir_override, max_log_files).await;

    // Check for and recover from an interrupted self-update BEFORE the TUI
    // comes up. Any failure here is non-fatal — we log and continue so a
    // broken lock file can't prevent the user from launching the app.
    let rollback_report = match update::install_root() {
        Ok(root) => match update::recover_from_interrupted_update(&root).await {
            Ok(report) => report,
            Err(e) => {
                tracing::warn!(error = %e, "failed to recover from interrupted update");
                None
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, "could not determine install root for update recovery");
            None
        }
    };

    // ratatui::init() sets up raw mode, alternate screen, and a panic hook.
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;
    let _guard = MouseGuard;

    let mut config = if let Some(c) = early_config {
        tracing::info!(
            org = c.azure_devops.organization,
            project = c.azure_devops.project,
            "app starting"
        );
        c
    } else {
        // No config file — run interactive setup inside the TUI.
        let result = azure_devops_cli::render::setup::run_setup(&mut terminal, &config_path).await;

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
    };

    let api_version = cli.api_version.clone();

    loop {
        let mut client = AdoClient::new(
            &config.azure_devops.organization,
            &config.azure_devops.project,
        )?;
        client.set_api_version(&api_version);

        tracing::info!(api_version = %api_version, "api client connected");

        let result = state::run::run(
            &mut terminal,
            client,
            &config,
            config_path.clone(),
            &api_version,
            log_init_status.clone(),
            rollback_report.clone(),
        )
        .await?;

        match result {
            state::run::RunResult::Quit => break,
            state::run::RunResult::Reload => {
                tracing::info!("reloading application");
                config = Config::load(Some(&config_path)).await?;
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
/// Logs go to `log_dir_override` if set, otherwise `~/.local/state/devops/`.
/// If the primary directory can't be created, falls back to a process-scoped
/// subdirectory of `std::env::temp_dir()`. If that also fails, logging is
/// disabled and the returned status lets the TUI surface the failure.
/// Retains up to `max_log_files` daily log files.
async fn init_tracing(
    level: &str,
    log_dir_override: Option<&str>,
    max_log_files: usize,
) -> LogInitStatus {
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
        .add_directive(
            "hyper_util=warn"
                .parse()
                .expect("static tracing directive must parse"),
        )
        .add_directive(
            "hyper=warn"
                .parse()
                .expect("static tracing directive must parse"),
        )
        .add_directive(
            "reqwest=warn"
                .parse()
                .expect("static tracing directive must parse"),
        )
        .add_directive(
            "mio=warn"
                .parse()
                .expect("static tracing directive must parse"),
        );

    // Compute the primary log directory. If the home dir is unavailable we
    // short-circuit to the fallback path below.
    let primary_dir: Option<PathBuf> = log_dir_override.map_or_else(
        || dirs::home_dir().map(|h| h.join(".local/state/devops")),
        |dir| Some(PathBuf::from(dir)),
    );

    let fallback_dir = || std::env::temp_dir().join(format!("devops-logs-{}", std::process::id()));

    let mut used_fallback = false;
    let log_dir = if let Some(dir) = &primary_dir {
        match tokio::fs::create_dir_all(dir).await {
            Ok(()) => dir.clone(),
            Err(e) => {
                eprintln!(
                    "warning: failed to create log directory {}: {e}; trying fallback",
                    dir.display()
                );
                used_fallback = true;
                fallback_dir()
            }
        }
    } else {
        eprintln!("warning: could not determine home directory; trying fallback log dir");
        used_fallback = true;
        fallback_dir()
    };

    if used_fallback && let Err(e) = tokio::fs::create_dir_all(&log_dir).await {
        eprintln!(
            "warning: failed to create fallback log directory {}: {e}; file logging disabled",
            log_dir.display()
        );
        return LogInitStatus::Failed;
    }

    let file_appender = rolling::RollingFileAppender::builder()
        .rotation(rolling::Rotation::DAILY)
        .filename_prefix("devops.log")
        .max_log_files(max_log_files)
        .build(&log_dir);

    let file_appender = match file_appender {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "warning: failed to create log appender in {}: {e}; file logging disabled",
                log_dir.display()
            );
            return LogInitStatus::Failed;
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
        fallback = used_fallback,
        "tracing initialized"
    );

    if used_fallback {
        LogInitStatus::Fallback(log_dir)
    } else {
        LogInitStatus::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// Clears env vars that might affect clap tests so they don't pick up the
    /// developer's actual environment.
    fn clear_env() {
        // SAFETY: `std::env::remove_var` is only `unsafe` because of threading
        // assumptions. The test binary runs these env-var mutations serially
        // within a single `#[test]` function (see below), so there are no
        // concurrent readers or writers of `DEVOPS_API_VERSION`.
        unsafe {
            std::env::remove_var("DEVOPS_API_VERSION");
        }
    }

    fn set_env(value: &str) {
        // SAFETY: see `clear_env`.
        unsafe {
            std::env::set_var("DEVOPS_API_VERSION", value);
        }
    }

    // All three assertions live in a single test to avoid cross-thread env
    // races when cargo runs tests in parallel inside one binary.
    #[test]
    fn api_version_flag_and_env_precedence() {
        // Default: no flag, no env → falls back to DEFAULT_API_VERSION ("7.1").
        clear_env();
        let cli = Cli::try_parse_from(["devops"]).expect("parse");
        assert_eq!(cli.api_version, "7.1");
        assert_eq!(DEFAULT_API_VERSION, "7.1");

        // Env var honored when flag is absent.
        set_env("8.0-preview.2");
        let cli = Cli::try_parse_from(["devops"]).expect("parse");
        assert_eq!(cli.api_version, "8.0-preview.2");

        // Flag overrides env var.
        set_env("8.0-preview.2");
        let cli = Cli::try_parse_from(["devops", "--api-version", "7.2-preview.3"]).expect("parse");
        assert_eq!(cli.api_version, "7.2-preview.3");

        clear_env();
    }
}
