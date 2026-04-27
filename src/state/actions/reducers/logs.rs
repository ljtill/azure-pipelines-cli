//! Log viewer reducer logic for timelines and log content.

use tokio::sync::mpsc;

use crate::client::http::AdoClient;
use crate::client::models::BuildTimeline;
use crate::components::log_viewer::ActiveTaskResult;
use crate::state::actions::spawn::spawn_log_fetch;
use crate::state::messages::AppMessage;
use crate::state::{App, TimelineRow};

use super::{LOG_REFRESH_BACKOFF_BASE_SECS, LOG_REFRESH_BACKOFF_MAX_SECS};

pub(in crate::state::actions) fn timeline(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    build_id: u32,
    timeline: BuildTimeline,
    generation: u64,
    is_refresh: bool,
) {
    // Discard stale timeline results.
    if generation != app.log_viewer.generation() {
        tracing::debug!(
            build_id,
            generation,
            expected = app.log_viewer.generation(),
            "discarding stale timeline"
        );
        return;
    }

    if is_refresh {
        tracing::debug!(
            build_id,
            records = timeline.records.len(),
            "timeline refreshed"
        );
    } else {
        tracing::info!(
            build_id,
            records = timeline.records.len(),
            "timeline loaded"
        );
    }

    app.log_viewer.set_build_timeline(timeline);

    // Update selected_build status from timeline data so the header stays current.
    app.log_viewer.refresh_build_status_from_timeline();

    if !is_refresh {
        setup_initial_timeline(app, client, tx, build_id);
    } else if app.log_viewer.is_following() {
        refresh_following_timeline(app, client, tx, build_id);
    } else {
        // Refresh in inspect mode: only update tree status, preserve cursor + log.
        app.log_viewer.rebuild_timeline_rows();
    }
}

pub(in crate::state::actions) fn log_content(
    app: &mut App,
    content: &str,
    generation: u64,
    log_id: u32,
) {
    // Discard stale log results.
    if generation != app.log_viewer.generation() {
        tracing::debug!(
            generation,
            expected = app.log_viewer.generation(),
            "discarding stale log content"
        );
        return;
    }
    // Discard log content for a different task when in follow mode.
    if app.log_viewer.is_following()
        && app
            .log_viewer
            .followed_log_id()
            .is_some_and(|followed_id| followed_id != log_id)
    {
        tracing::debug!(
            log_id,
            followed = ?app.log_viewer.followed_log_id(),
            "discarding log content for non-followed task"
        );
        return;
    }
    tracing::debug!(bytes = content.len(), log_id, "log content received");
    app.log_viewer.set_log_content(content);
}

pub(in crate::state::actions) fn log_refresh_finished(app: &mut App, had_failure: bool) {
    tracing::debug!(had_failure, "log refresh finished");
    if app.refresh.log_refresh.in_flight {
        if had_failure {
            app.refresh
                .log_refresh
                .fail(LOG_REFRESH_BACKOFF_BASE_SECS, LOG_REFRESH_BACKOFF_MAX_SECS);
        } else {
            app.refresh.log_refresh.succeed();
        }
    } else {
        tracing::debug!("ignoring log refresh finish because no refresh is in flight");
    }
}

fn setup_initial_timeline(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    build_id: u32,
) {
    // Initial load: full setup with auto-select.
    app.log_viewer.clear_log();
    app.log_viewer.nav_mut().set_index(0);
    app.log_viewer.enter_follow_mode();
    app.log_viewer.rebuild_timeline_rows();

    if let Some((_index, maybe_log_id)) = app.log_viewer.auto_select_log_entry() {
        let task_name = if let Some(TimelineRow::Task { name, .. }) = app
            .log_viewer
            .timeline_rows()
            .get(app.log_viewer.nav().index())
        {
            name.clone()
        } else {
            String::new()
        };

        if let Some(log_id) = maybe_log_id {
            app.log_viewer.set_followed(task_name, log_id);
            let generation = app.log_viewer.generation();
            spawn_log_fetch(app, client, tx, build_id, log_id, generation);
        } else {
            // In-progress task has no log yet — show it but wait for log.
            app.log_viewer.set_followed_pending(task_name);
        }
    }
}

fn refresh_following_timeline(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    build_id: u32,
) {
    // Refresh in follow mode: update tree, track latest active task.
    app.log_viewer.rebuild_timeline_rows();

    match app.log_viewer.find_active_task() {
        ActiveTaskResult::Found { name, log_id } => {
            let task_changed = app.log_viewer.followed_log_id() != Some(log_id);
            app.log_viewer.set_followed(name, log_id);

            if task_changed {
                tracing::debug!(build_id, log_id, "follow mode: task changed");
                app.log_viewer.jump_to_followed_task();
                app.log_viewer.clear_log();
                let generation = app.log_viewer.generation();
                spawn_log_fetch(app, client, tx, build_id, log_id, generation);
            }
        }
        ActiveTaskResult::Pending { name } => {
            // The next step is starting — jump cursor to it, clear the
            // log pane, and keep follow mode active until the log appears.
            tracing::debug!(
                build_id,
                task = %name,
                "follow mode: task pending log"
            );
            app.log_viewer.set_followed_pending(name);
            app.log_viewer.jump_to_followed_task();
            app.log_viewer.clear_log();
        }
        ActiveTaskResult::None => {
            // Build completed or no active task — exit follow mode.
            tracing::debug!(
                build_id,
                "follow mode: no active task, switching to inspect"
            );
            app.log_viewer.enter_inspect_mode();
        }
    }
}
