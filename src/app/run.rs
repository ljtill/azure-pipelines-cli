use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{EventStream, KeyEventKind};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use crate::api::client::AdoClient;
use crate::config::{Config, LOG_REFRESH_INTERVAL_SECS, REFRESH_INTERVAL_SECS};
use crate::events::{handle_key, handle_mouse};
use crate::ui;

use super::actions::{handle_action, handle_message, spawn_data_refresh, spawn_log_refresh};
use super::messages::AppMessage;
use super::{App, View};

pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    client: AdoClient,
    config: &Config,
) -> Result<()> {
    let mut app = App::new(
        &config.azure_devops.organization,
        &config.azure_devops.project,
        config,
    );
    let refresh_interval = Duration::from_secs(REFRESH_INTERVAL_SECS);
    let log_refresh_interval = Duration::from_secs(LOG_REFRESH_INTERVAL_SECS);
    let mut last_data_fetch = Instant::now() - refresh_interval; // trigger immediate fetch
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

    tracing::info!(refresh_secs = REFRESH_INTERVAL_SECS, "event loop starting");

    loop {
        if !app.running {
            break;
        }

        // Spawn periodic background refreshes
        let should_refresh_data = !app.data_refresh_in_flight
            && app
                .data_refresh_backoff_until
                .map(|until| Instant::now() >= until)
                .unwrap_or(true)
            && last_data_fetch.elapsed() >= refresh_interval;
        let should_refresh_logs = app.view == View::LogViewer
            && app.log_viewer.selected_build().is_some()
            && !app.log_refresh_in_flight
            && app
                .log_refresh_backoff_until
                .map(|until| Instant::now() >= until)
                .unwrap_or(true)
            && last_log_fetch
                .map(|t| t.elapsed() >= log_refresh_interval)
                .unwrap_or(true);

        if should_refresh_data && spawn_data_refresh(&mut app, &client, &tx) {
            last_data_fetch = Instant::now();
        }

        if should_refresh_logs && spawn_log_refresh(&mut app, &client, &tx) {
            last_log_fetch = Some(Instant::now());
        }

        // Draw
        terminal.draw(|f| ui::draw(f, &app))?;

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

    Ok(())
}
