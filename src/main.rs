mod api;
mod app;
mod config;
mod events;
#[cfg(test)]
mod test_helpers;
mod ui;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::api::client::AdoClient;
use crate::config::Config;

#[derive(Parser)]
#[command(
    name = "azure-pipelines-cli",
    about = "TUI dashboard for Azure DevOps Pipelines"
)]
struct Cli {
    /// Path to config file
    #[arg(short, long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing with file-based output (avoids polluting the TUI).
    // Controlled via RUST_LOG env var (e.g. RUST_LOG=debug).
    init_tracing();

    // Panic hook to restore terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stderr(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    let cli = Cli::parse();
    let config = Config::load(cli.config.as_ref())?;

    let client = AdoClient::new(
        &config.azure_devops.organization,
        &config.azure_devops.project,
    )
    .await?;

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = app::run::run(&mut terminal, client, &config).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Initialize tracing to log to a file (if RUST_LOG is set).
/// Logs go to `~/.local/state/azure-pipelines-cli/debug.log`.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt;
    use tracing_subscriber::prelude::*;

    let filter = EnvFilter::try_from_default_env();
    let Ok(filter) = filter else {
        // RUST_LOG not set — skip logging entirely
        return;
    };

    let log_dir = dirs::home_dir()
        .map(|h| h.join(".local/state/azure-pipelines-cli"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("debug.log");

    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
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
