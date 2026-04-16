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

use super::actions::spawn::{
    spawn_fetch_boards, spawn_fetch_dashboard_pull_requests, spawn_fetch_pull_requests,
    spawn_fetch_user_identity,
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

pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    client: AdoClient,
    config: &Config,
    config_path: std::path::PathBuf,
) -> Result<RunResult> {
    let mut app = App::new(
        &config.azure_devops.organization,
        &config.azure_devops.project,
        config,
        config_path,
    );
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
    if config.update.check_for_updates {
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
        refresh_secs = config.display.refresh_interval_secs,
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
