use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{EventStream, KeyEventKind};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use crate::api::client::AdoClient;
use crate::config::Config;
use crate::events::handle_key;
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
    let refresh_interval = Duration::from_secs(config.display.refresh_interval_secs);
    let log_refresh_interval = Duration::from_secs(config.display.log_refresh_interval_secs);
    let mut last_data_fetch = Instant::now() - refresh_interval; // trigger immediate fetch
    let mut last_log_fetch: Option<Instant> = None;

    let (tx, mut rx) = mpsc::channel::<AppMessage>(64);
    let mut event_stream = EventStream::new();

    loop {
        if !app.running {
            break;
        }

        // Spawn periodic background refreshes
        let should_refresh_data = last_data_fetch.elapsed() >= refresh_interval;
        let should_refresh_logs = app.view == View::LogViewer
            && app.log_viewer.selected_build.is_some()
            && last_log_fetch
                .map(|t| t.elapsed() >= log_refresh_interval)
                .unwrap_or(true);

        if should_refresh_data {
            spawn_data_refresh(&client, &tx);
            last_data_fetch = Instant::now();
        }

        if should_refresh_logs {
            spawn_log_refresh(&app, &client, &tx);
            last_log_fetch = Some(Instant::now());
        }

        // Draw
        terminal.draw(|f| ui::draw(f, &app))?;

        // Async event stream: no dropped keypresses since EventStream only
        // consumes from crossterm's buffer when the future completes.
        tokio::select! {
            Some(event_result) = event_stream.next() => {
                if let Ok(crossterm::event::Event::Key(key)) = event_result
                    && key.kind == KeyEventKind::Press
                {
                    let action = handle_key(&mut app, key);
                    handle_action(&mut app, &client, &tx, action, &mut last_data_fetch);
                }
            }

            Some(msg) = rx.recv() => {
                handle_message(&mut app, &client, &tx, msg);
            }
        }
    }

    Ok(())
}
