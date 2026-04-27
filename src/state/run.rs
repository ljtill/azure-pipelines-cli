//! Main event loop driving terminal rendering and async message dispatch.

use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Utc;
use crossterm::event::{
    Event as CrosstermEvent, EventStream, KeyEventKind, MouseEvent, MouseEventKind,
};
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
    send_app_message, spawn_fetch_boards, spawn_fetch_dashboard_pull_requests,
    spawn_fetch_pinned_work_items, spawn_fetch_pull_requests, spawn_fetch_user_identity,
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

#[derive(Debug)]
struct RedrawScheduler {
    needed: bool,
}

impl RedrawScheduler {
    fn new() -> Self {
        Self { needed: true }
    }

    fn request(&mut self) {
        self.needed = true;
    }

    fn take(&mut self) -> bool {
        std::mem::take(&mut self.needed)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TickSnapshot {
    refresh_label: Option<String>,
    transient_notification_visible: bool,
}

impl TickSnapshot {
    fn capture(app: &App) -> Self {
        Self {
            refresh_label: refresh_tick_label(app),
            transient_notification_visible: app
                .notifications
                .clone_current()
                .is_some_and(|notification| !notification.persistent),
        }
    }

    fn changed_since(&self, app: &App) -> bool {
        self != &Self::capture(app)
    }
}

fn refresh_tick_label(app: &App) -> Option<String> {
    if app.refresh.loading {
        return None;
    }

    app.refresh.last_refresh.map(|last| {
        let elapsed = Utc::now().signed_duration_since(last);
        if elapsed.num_seconds() < 60 {
            format!("{}s", elapsed.num_seconds())
        } else {
            format!("{}m", elapsed.num_minutes())
        }
    })
}

fn mouse_affects_visible_output(app: &App, mouse: MouseEvent) -> bool {
    matches!(
        mouse.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) && app.view == View::LogViewer
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
        .checked_sub(app.refresh.refresh_interval)
        .unwrap_or_else(Instant::now);
    let mut last_log_fetch: Option<Instant> = None;

    let (tx, mut rx) = mpsc::channel::<AppMessage>(64);
    let mut event_stream = EventStream::new();
    let mut ui_tick = tokio::time::interval(Duration::from_secs(1));
    let mut redraw = RedrawScheduler::new();
    let mut tick_snapshot = TickSnapshot::default();

    // Spawn background update check (once, at startup).
    if config.devops.update.check_for_updates {
        let tx = tx.clone();
        tokio::spawn(async move {
            if let Some(version) = crate::update::check_for_update().await {
                send_app_message(&tx, "update_check", AppMessage::UpdateAvailable { version })
                    .await;
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
        app.refresh.effects.prune_finished();
        let should_refresh_data = !app.refresh.data_refresh.in_flight
            && app.refresh.data_refresh.backoff_elapsed()
            && last_data_fetch.elapsed() >= app.refresh.refresh_interval;
        let should_refresh_logs = app.view == View::LogViewer
            && app.log_viewer.selected_build().is_some()
            && !app.refresh.log_refresh.in_flight
            && app.refresh.log_refresh.backoff_elapsed()
            && last_log_fetch.is_none_or(|t| t.elapsed() >= app.refresh.log_refresh_interval);

        let mut refresh_started = false;
        if should_refresh_data && spawn_data_refresh(&mut app, &client, &tx) {
            last_data_fetch = Instant::now();
            refresh_started = true;
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
                spawn_fetch_pull_requests(&mut app, &client, &tx, generation);
            }
            if app.view == View::Boards {
                let generation = app.boards.next_generation();
                spawn_fetch_boards(&mut app, &client, &tx, generation);
            }
        }

        if should_refresh_logs && spawn_log_refresh(&mut app, &client, &tx) {
            last_log_fetch = Some(Instant::now());
            refresh_started = true;
        }

        if refresh_started {
            redraw.request();
        }

        if redraw.take() {
            terminal.draw(|f| render::draw(f, &mut app))?;
            tick_snapshot = TickSnapshot::capture(&app);
        }

        // Async event stream: no dropped keypresses since EventStream only
        // consumes from crossterm's buffer when the future completes.
        tokio::select! {
            Some(event_result) = event_stream.next() => {
                match event_result {
                    Ok(CrosstermEvent::Key(key)) if key.kind == KeyEventKind::Press => {
                        let action = handle_key(&mut app, key);
                        handle_action(&mut app, &client, &tx, action, &mut last_data_fetch);
                        redraw.request();
                    }
                    Ok(CrosstermEvent::Mouse(mouse)) => {
                        let affects_output = mouse_affects_visible_output(&app, mouse);
                        let action = handle_mouse(&mut app, mouse);
                        handle_action(&mut app, &client, &tx, action, &mut last_data_fetch);
                        if affects_output {
                            redraw.request();
                        }
                    }
                    Ok(CrosstermEvent::Resize(_, _)) => {
                        redraw.request();
                    }
                    _ => {}
                }
            }

            Some(msg) = rx.recv() => {
                handle_message(&mut app, &client, &tx, msg);
                redraw.request();
            }

            // Periodic UI tick: keeps the "Xs ago" counter and refresh
            // scheduling alive without requiring user input.
            _ = ui_tick.tick() => {
                if tick_snapshot.changed_since(&app) {
                    redraw.request();
                }
            }
        }
    }

    tracing::info!("app shutting down");
    app.refresh.effects.cancel_all();

    if app.reload_requested {
        Ok(RunResult::Reload)
    } else {
        Ok(RunResult::Quit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseEvent};

    #[test]
    fn redraw_scheduler_starts_dirty_then_clears() {
        let mut redraw = RedrawScheduler::new();

        assert!(redraw.take());
        assert!(!redraw.take());
    }

    #[test]
    fn redraw_scheduler_sets_dirty_when_requested() {
        let mut redraw = RedrawScheduler::new();
        assert!(redraw.take());

        redraw.request();

        assert!(redraw.take());
        assert!(!redraw.take());
    }

    #[test]
    fn tick_snapshot_tracks_refresh_age() {
        let mut app = crate::test_helpers::make_app();
        let snapshot = TickSnapshot::capture(&app);

        app.refresh.last_refresh = Some(Utc::now());

        assert!(snapshot.changed_since(&app));
        assert!(TickSnapshot::capture(&app).refresh_label.is_some());
    }

    #[test]
    fn tick_snapshot_tracks_transient_notifications() {
        let mut app = crate::test_helpers::make_app();
        let snapshot = TickSnapshot::capture(&app);

        app.notifications.success("Saved");

        assert!(snapshot.changed_since(&app));
    }

    #[test]
    fn tick_snapshot_ignores_persistent_notifications() {
        let mut app = crate::test_helpers::make_app();
        let snapshot = TickSnapshot::capture(&app);

        app.notifications
            .push_persistent(NotificationLevel::Info, "Still here");

        assert!(!snapshot.changed_since(&app));
    }

    #[test]
    fn mouse_scroll_only_affects_log_viewer_output() {
        let mut app = crate::test_helpers::make_app();
        let mouse = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };

        assert!(!mouse_affects_visible_output(&app, mouse));

        app.view = View::LogViewer;

        assert!(mouse_affects_visible_output(&app, mouse));
    }
}
