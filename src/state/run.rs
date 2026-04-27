//! Main event loop driving terminal rendering and async message dispatch.

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{EventStream, KeyEventKind};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use crate::client::http::AdoClient;
use crate::config::Config;
use crate::events::{handle_key, handle_mouse};
use crate::render;
use crate::state::notifications::NotificationLevel;

use super::actions::spawn::{
    spawn_fetch_boards, spawn_fetch_dashboard_pull_requests, spawn_fetch_pinned_work_items,
    spawn_fetch_pull_requests, spawn_fetch_user_identity,
};
use super::actions::{handle_action, handle_message, spawn_data_refresh, spawn_log_refresh};
use super::messages::AppMessage;
use super::{App, View};

/// Represents the outcome of the run loop — tells the caller whether to quit or reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunResult {
    Quit,
    Reload,
}

/// Represents the result of initializing file-based logging.
///
/// Carried from `main` into the TUI run loop so the user-facing notification
/// queue can surface log-init failures even though stderr is hidden by the TUI.
#[derive(Debug, Clone)]
pub enum LogInitStatus {
    /// Logging initialized at the primary log directory.
    Ok,
    /// Primary log directory creation failed; logging fell back to a temp-dir path.
    Fallback(std::path::PathBuf),
    /// Logging is disabled because both primary and fallback directories failed.
    Failed,
}

pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    client: AdoClient,
    config: &Config,
    config_path: std::path::PathBuf,
    api_version: &str,
    log_init_status: LogInitStatus,
    rollback_report: Option<crate::update::RollbackReport>,
) -> Result<RunResult> {
    let mut app = App::new(
        &config.devops.connection.organization,
        &config.devops.connection.project,
        config,
        config_path,
    );
    app.set_api_version(api_version);

    // Surface any log-init failure in the UI, since stderr is hidden by the TUI.
    match &log_init_status {
        LogInitStatus::Ok => {}
        LogInitStatus::Fallback(path) => {
            app.notifications.push_persistent(
                NotificationLevel::Info,
                format!(
                    "Logging fell back to temporary directory: {}. Diagnostics may be rotated away on reboot.",
                    path.display()
                ),
            );
        }
        LogInitStatus::Failed => {
            app.notifications.push_persistent(
                NotificationLevel::Error,
                "Logging disabled (failed to create log directory). Diagnostics will not be available.",
            );
        }
    }

    // Surface any startup rollback from a previously-interrupted self-update.
    if let Some(report) = rollback_report {
        app.notifications.push_persistent(
            NotificationLevel::Error,
            format!(
                "Previous update from v{} to v{} was interrupted and rolled back.",
                report.from_version, report.to_version
            ),
        );
    }

    // Trigger an immediate fetch on startup. Fall back to `now` if the refresh
    // interval is larger than the system's uptime (e.g. on a freshly-booted host
    // or after a clock adjustment).
    let mut last_data_fetch = Instant::now()
        .checked_sub(app.refresh_interval)
        .unwrap_or_else(Instant::now);
    let mut last_log_fetch: Option<Instant> = None;

    let (tx, mut rx) = mpsc::channel::<AppMessage>(64);
    let mut event_stream = EventStream::new();
    let mut ui_tick = tokio::time::interval(Duration::from_secs(1));

    // Spawn background update check (once, at startup).
    if config.devops.update.check_for_updates {
        let tx = tx.clone();
        tokio::spawn(async move {
            if let Some(version) = crate::update::check_for_update().await {
                let _ = tx.send(AppMessage::UpdateAvailable { version }).await;
            }
        });
    }

    // Resolve current user identity for PR filtering (once, at startup).
    spawn_fetch_user_identity(&client, &tx);

    tracing::info!(
        refresh_secs = config.devops.display.refresh_interval_secs,
        "event loop starting"
    );

    loop {
        if !app.running {
            break;
        }

        // Spawn periodic background refreshes.
        let should_refresh_data = !app.data_refresh.in_flight
            && app.data_refresh.backoff_elapsed()
            && last_data_fetch.elapsed() >= app.refresh_interval;
        let should_refresh_logs = app.view == View::LogViewer
            && app.log_viewer.selected_build().is_some()
            && !app.log_refresh.in_flight
            && app.log_refresh.backoff_elapsed()
            && last_log_fetch.is_none_or(|t| t.elapsed() >= app.log_refresh_interval);

        if should_refresh_data && spawn_data_refresh(&mut app, &client, &tx) {
            last_data_fetch = Instant::now();
            // Refresh dashboard PRs alongside the data refresh. If identity is not
            // ready yet, this re-attempts identity resolution instead of showing
            // unverified pull requests.
            if app.view == View::Dashboard {
                spawn_fetch_dashboard_pull_requests(&mut app, &client, &tx);
                spawn_fetch_pinned_work_items(&mut app, &client, &tx);
            }
            // Refresh PR view data alongside the data refresh.
            if app.view.is_pull_requests() {
                let generation = app.pull_requests.next_generation();
                spawn_fetch_pull_requests(&app, &client, &tx, generation);
            }
            if app.view == View::Boards {
                let generation = app.boards.next_generation();
                spawn_fetch_boards(&mut app, &client, &tx, generation);
            }
        }

        if should_refresh_logs && spawn_log_refresh(&mut app, &client, &tx) {
            last_log_fetch = Some(Instant::now());
        }

        // Draw.
        terminal.draw(|f| render::draw(f, &mut app))?;

        // Async event stream: no dropped keypresses since EventStream only
        // consumes from crossterm's buffer when the future completes.
        tokio::select! {
            Some(event_result) = event_stream.next() => {
                match event_result {
                    Ok(crossterm::event::Event::Key(key)) if key.kind == KeyEventKind::Press => {
                        let action = handle_key(&mut app, key);
                        handle_action(&mut app, &client, &tx, action, &mut last_data_fetch);
                    }
                    Ok(crossterm::event::Event::Mouse(mouse)) => {
                        let action = handle_mouse(&mut app, mouse);
                        handle_action(&mut app, &client, &tx, action, &mut last_data_fetch);
                    }
                    _ => {}
                }
            }

            Some(msg) = rx.recv() => {
                handle_message(&mut app, &client, &tx, msg);
            }

            // Periodic UI tick: keeps the "Xs ago" counter and refresh
            // scheduling alive without requiring user input.
            _ = ui_tick.tick() => {}
        }
    }

    tracing::info!("app shutting down");

    if app.reload_requested {
        Ok(RunResult::Reload)
    } else {
        Ok(RunResult::Quit)
    }
}
