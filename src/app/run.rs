use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{EventStream, KeyEventKind};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use crate::api::client::AdoClient;
use crate::config::Config;
use crate::events::{handle_key, handle_mouse};
use crate::ui;

use super::actions::{handle_action, handle_message, spawn_data_refresh, spawn_log_refresh};
use super::messages::AppMessage;
use super::{App, View};

/// Outcome of the run loop — tells the caller whether to quit or reload.
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
    let mut last_data_fetch = Instant::now() - app.refresh_interval; // trigger immediate fetch
    let mut last_log_fetch: Option<Instant> = None;

    let (tx, mut rx) = mpsc::channel::<AppMessage>(64);
    let mut event_stream = EventStream::new();
    let mut ui_tick = tokio::time::interval(Duration::from_secs(1));

    // Spawn background update check (once, at startup)
    if config.update.check_for_updates {
        let tx = tx.clone();
        tokio::spawn(async move {
            if let Some(version) = crate::update::check_for_update().await {
                let _ = tx.send(AppMessage::UpdateAvailable { version }).await;
            }
        });
    }

    tracing::info!(
        refresh_secs = config.display.refresh_interval_secs,
        "event loop starting"
    );

    loop {
        if !app.running {
            break;
        }

        // Spawn periodic background refreshes
        let should_refresh_data = !app.data_refresh.in_flight
            && app.data_refresh.backoff_elapsed()
            && last_data_fetch.elapsed() >= app.refresh_interval;
        let should_refresh_logs = app.view == View::LogViewer
            && app.log_viewer.selected_build().is_some()
            && !app.log_refresh.in_flight
            && app.log_refresh.backoff_elapsed()
            && last_log_fetch
                .map(|t| t.elapsed() >= app.log_refresh_interval)
                .unwrap_or(true);

        if should_refresh_data && spawn_data_refresh(&mut app, &client, &tx) {
            last_data_fetch = Instant::now();
        }

        if should_refresh_logs && spawn_log_refresh(&mut app, &client, &tx) {
            last_log_fetch = Some(Instant::now());
        }

        // Draw
        terminal.draw(|f| ui::draw(f, &mut app))?;

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
