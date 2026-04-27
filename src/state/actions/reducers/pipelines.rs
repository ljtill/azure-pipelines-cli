//! Pipelines reducer logic for refreshes, build history, and run mutations.

use tokio::sync::mpsc;

use crate::client::http::AdoClient;
use crate::client::models::{
    self, Approval, Build, BuildResult, PipelineDefinition, RetentionLease,
};
use crate::shared::availability::Availability;
use crate::state::actions::spawn::{
    spawn_build_history_refresh, spawn_data_refresh, spawn_timeline_fetch,
};
use crate::state::messages::{AppMessage, RefreshOutcome};
use crate::state::notifications::NotificationLevel;
use crate::state::{App, CoreDataSnapshot, View};

use super::{DATA_REFRESH_BACKOFF_BASE_SECS, DATA_REFRESH_BACKOFF_MAX_SECS};

pub(in crate::state::actions) fn data_refresh(
    app: &mut App,
    definitions: RefreshOutcome<Vec<PipelineDefinition>>,
    recent_builds: RefreshOutcome<Vec<Build>>,
    pending_approvals: RefreshOutcome<Vec<Approval>>,
    retention_leases: RefreshOutcome<Vec<RetentionLease>>,
) {
    let definitions = resolve_refresh_outcome(
        &app.core.data.definitions,
        &app.core.availability.definitions,
        definitions,
    );
    let recent_builds = resolve_refresh_outcome(
        &app.core.data.recent_builds,
        &app.core.availability.recent_builds,
        recent_builds,
    );
    let pending_approvals = resolve_refresh_outcome(
        &app.core.data.pending_approvals,
        &app.core.availability.pending_approvals,
        pending_approvals,
    );
    let retention_leases = resolve_refresh_outcome(
        &app.core.retention_leases.leases,
        &app.core.availability.retention_leases,
        retention_leases,
    );

    let errors: Vec<String> = [
        definitions.error.clone(),
        recent_builds.error.clone(),
        pending_approvals.error.clone(),
        retention_leases.error.clone(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let any_fresh = definitions.fresh
        || recent_builds.fresh
        || pending_approvals.fresh
        || retention_leases.fresh;
    let primary_refresh_failed = !definitions.fresh && !recent_builds.fresh;
    let definitions_availability = definitions.availability.clone();
    let recent_builds_availability = recent_builds.availability.clone();
    let pending_approvals_availability = pending_approvals.availability.clone();
    let retention_leases_availability = retention_leases.availability.clone();

    if primary_refresh_failed && app.refresh.data_refresh.in_flight {
        app.refresh.data_refresh.fail(
            DATA_REFRESH_BACKOFF_BASE_SECS,
            DATA_REFRESH_BACKOFF_MAX_SECS,
        );
    } else if primary_refresh_failed {
        tracing::debug!("ignoring data refresh failure because no refresh is in flight");
    } else {
        app.refresh.data_refresh.succeed();
    }

    for error in &errors {
        tracing::warn!(error = %error, "data section refresh failed");
        app.notifications.error_dedup(error.clone());
    }

    app.core
        .data
        .apply_refresh(definitions.data, recent_builds.data, pending_approvals.data);
    app.core.retention_leases.set_leases(retention_leases.data);
    app.core.availability.definitions = definitions_availability;
    app.core.availability.recent_builds = recent_builds_availability;
    app.core.availability.pending_approvals = pending_approvals_availability;
    app.core.availability.retention_leases = retention_leases_availability;

    let snapshot = CoreDataSnapshot::from_data(&app.core.data, &app.core.retention_leases);
    let had_refresh_snapshot = app.core.availability.refresh.data().is_some()
        || snapshot.definitions > 0
        || snapshot.recent_builds > 0
        || snapshot.pending_approvals > 0
        || snapshot.retention_leases > 0;
    app.core.availability.refresh = if errors.is_empty() {
        Availability::fresh(snapshot)
    } else if any_fresh {
        Availability::partial(snapshot, errors.clone())
    } else {
        let message = errors.join("; ");
        if had_refresh_snapshot {
            Availability::stale(snapshot, message)
        } else {
            Availability::unavailable(message)
        }
    };

    let latest_snapshot: Vec<(u32, Build)> = app
        .core
        .data
        .latest_builds_by_def
        .iter()
        .map(|(&definition_id, build)| (definition_id, build.clone()))
        .collect();

    tracing::info!(
        definitions = app.core.data.definitions.len(),
        active = app.core.data.active_builds.len(),
        recent = app.core.data.recent_builds.len(),
        approvals = app.core.data.pending_approvals.len(),
        retention = app.core.retention_leases.leases.len(),
        degraded_sections = errors.len(),
        "data refresh received"
    );

    emit_latest_build_notifications(app, &latest_snapshot);
    app.refresh.prev_latest_builds = latest_snapshot
        .iter()
        .map(|(definition_id, build)| (*definition_id, (build.id, build.status, build.result)))
        .collect();

    app.rebuild_dashboard();
    app.rebuild_pipelines();
    let query = app.search.query.clone();
    app.shell.views.active_runs.rebuild(
        &app.core.data.active_builds,
        &app.core.filters.definition_ids,
        &query,
    );
    if any_fresh {
        app.refresh.last_refresh = Some(chrono::Utc::now());
    }
    app.refresh.loading = false;

    // Terminal message for the data refresh — clear any in-flight
    // pagination progress so the header doesn't linger stale state.
    app.refresh.pagination_status = None;
}

struct SectionData<T> {
    data: Vec<T>,
    fresh: bool,
    error: Option<String>,
    availability: Availability<Vec<T>>,
}

fn resolve_refresh_outcome<T: Clone>(
    current_data: &[T],
    availability: &Availability<Vec<T>>,
    outcome: RefreshOutcome<Vec<T>>,
) -> SectionData<T> {
    match outcome {
        RefreshOutcome::Fresh(data) => SectionData {
            availability: Availability::fresh(data.clone()),
            data,
            fresh: true,
            error: None,
        },
        RefreshOutcome::Partial { data, errors } => {
            let error = if errors.is_empty() {
                None
            } else {
                Some(errors.join("; "))
            };
            let availability = Availability::partial(data.clone(), errors);
            SectionData {
                data,
                fresh: true,
                error,
                availability,
            }
        }
        RefreshOutcome::Failed { message } => {
            let previous_data = availability.data().cloned();
            let had_previous = previous_data.is_some() || !current_data.is_empty();
            let data = previous_data.unwrap_or_else(|| current_data.to_vec());
            let availability = if had_previous {
                Availability::stale(data.clone(), message.clone())
            } else {
                Availability::unavailable(message.clone())
            };
            SectionData {
                data,
                fresh: false,
                error: Some(message),
                availability,
            }
        }
    }
}

pub(in crate::state::actions) fn build_history(
    app: &mut App,
    builds: Vec<Build>,
    continuation_token: Option<String>,
    generation: u64,
) {
    if generation != app.build_history.generation {
        tracing::debug!(
            generation,
            expected = app.build_history.generation,
            "dropping obsolete build history response"
        );
        return;
    }
    tracing::info!(
        count = builds.len(),
        has_more = continuation_token.is_some(),
        "build history loaded"
    );
    app.build_history.has_more = continuation_token.is_some();
    app.build_history.continuation_token = continuation_token;
    app.build_history.builds = builds;
    let build_count = app.build_history.builds.len();
    app.build_history.nav.set_len(build_count);
    // Restore stashed scroll position (e.g. after lease deletion refresh).
    if let Some(index) = app.build_history.pending_nav_index.take() {
        app.build_history.nav.set_index(index);
    }
    app.refresh.pagination_status = None;
}

pub(in crate::state::actions) fn build_history_more(
    app: &mut App,
    builds: Vec<Build>,
    continuation_token: Option<String>,
    generation: u64,
) {
    if generation != app.build_history.generation {
        tracing::debug!(
            generation,
            expected = app.build_history.generation,
            "dropping obsolete build history pagination response"
        );
        return;
    }
    tracing::info!(
        count = builds.len(),
        has_more = continuation_token.is_some(),
        "more build history loaded"
    );
    app.build_history.loading_more = false;
    app.build_history.has_more = continuation_token.is_some();
    app.build_history.continuation_token = continuation_token;
    app.build_history.builds.extend(builds);
    let build_count = app.build_history.builds.len();
    app.build_history.nav.set_len(build_count);
    app.refresh.pagination_status = None;
}

pub(in crate::state::actions) fn build_cancelled(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
) {
    tracing::info!("build cancelled successfully");
    app.notifications.success("Build cancelled");
    spawn_data_refresh(app, client, tx);
    if app.view == View::BuildHistory {
        spawn_build_history_refresh(app, client, tx, None);
    }
    if let Some(build) = app.log_viewer.selected_build() {
        let build_id = build.id;
        let generation = app.log_viewer.generation();
        spawn_timeline_fetch(app, client, tx, build_id, generation, true);
    }
}

pub(in crate::state::actions) fn builds_cancelled(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    cancelled: u32,
    failed: u32,
) {
    tracing::info!(cancelled, failed, "builds cancelled");
    app.active_runs.selected.clear();
    spawn_data_refresh(app, client, tx);
    if app.view == View::BuildHistory {
        spawn_build_history_refresh(app, client, tx, None);
    }
    if failed > 0 {
        app.notifications
            .error(format!("Cancelled {cancelled}, {failed} failed"));
    } else {
        app.notifications
            .success(format!("Cancelled {cancelled} build(s)"));
    }
}

pub(in crate::state::actions) fn stage_retried(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
) {
    tracing::info!("stage retried successfully");
    app.notifications.success("Stage retried");
    if let Some(build) = app.log_viewer.selected_build() {
        let build_id = build.id;
        let generation = app.log_viewer.generation();
        spawn_timeline_fetch(app, client, tx, build_id, generation, true);
    }
    spawn_data_refresh(app, client, tx);
}

pub(in crate::state::actions) fn check_updated(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
) {
    tracing::info!("check updated successfully");
    app.notifications.success("Check updated");
    spawn_data_refresh(app, client, tx);
    if let Some(build) = app.log_viewer.selected_build() {
        let build_id = build.id;
        let generation = app.log_viewer.generation();
        spawn_timeline_fetch(app, client, tx, build_id, generation, true);
    }
}

pub(in crate::state::actions) fn pipeline_queued(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    build: Build,
) {
    tracing::info!(build_id = build.id, "pipeline queued");
    let build_id = build.id;
    app.navigate_to_log_viewer(build);
    let generation = app.log_viewer.generation();
    spawn_timeline_fetch(app, client, tx, build_id, generation, false);
}

pub(in crate::state::actions) fn retention_leases_deleted(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    deleted: u32,
    failed: u32,
) {
    tracing::info!(deleted, failed, "retention leases deleted");
    if failed > 0 {
        app.notifications
            .error(format!("Deleted {deleted} lease(s), {failed} failed"));
    } else {
        app.notifications
            .success(format!("Deleted {deleted} retention lease(s)"));
    }
    // Clear stale multi-select state and preserve scroll position.
    app.build_history.selected.clear();
    app.build_history.pending_nav_index = Some(app.build_history.nav.index());
    // Trigger a full data refresh to re-fetch leases.
    spawn_data_refresh(app, client, tx);
    if app.view == View::BuildHistory {
        // Request enough builds to cover everything already loaded so the
        // scroll position can be restored after the refresh.
        let top = (app.build_history.builds.len() as u32)
            .max(crate::client::endpoints::TOP_DEFINITION_BUILDS);
        spawn_build_history_refresh(app, client, tx, Some(top));
    }
}

fn emit_latest_build_notifications(app: &mut App, latest_builds: &[(u32, Build)]) {
    // Detect build state changes and emit in-app notifications.
    // Fires when:
    //   - A build transitions to InProgress (started).
    //   - A build transitions to Completed (succeeded/failed/canceled).
    // Skipped on first load (prev is empty) to avoid a startup storm.
    if !app.refresh.notifications_enabled || app.refresh.prev_latest_builds.is_empty() {
        return;
    }

    for (definition_id, build) in latest_builds {
        let prev = app.refresh.prev_latest_builds.get(definition_id);
        let (prev_id, prev_status) = match prev {
            Some(&(id, status, _)) => (Some(id), Some(status)),
            None => (None, None),
        };

        let id_changed = prev_id != Some(build.id);
        let status_changed = prev_status != Some(build.status);

        // Only notify on meaningful transitions.
        if !id_changed && !status_changed {
            continue;
        }

        if build.status == models::BuildStatus::InProgress {
            let message = format!("{} #{} started", build.definition.name, build.build_number);
            tracing::info!(
                definition = build.definition.name,
                build_id = build.id,
                "pipeline started"
            );
            app.notifications.push(NotificationLevel::Info, message);
        } else if build.status == models::BuildStatus::Completed {
            emit_completed_build_notification(app, build);
        }
    }
}

fn emit_completed_build_notification(app: &mut App, build: &Build) {
    let result_label = match build.result {
        Some(BuildResult::Succeeded) => "succeeded",
        Some(BuildResult::PartiallySucceeded) => "partially succeeded",
        Some(BuildResult::Failed) => "failed",
        Some(BuildResult::Canceled) => "canceled",
        _ => "completed",
    };
    let message = format!(
        "{} #{} {}",
        build.definition.name, build.build_number, result_label
    );
    let level = match build.result {
        Some(BuildResult::Succeeded) => NotificationLevel::Success,
        Some(BuildResult::Failed | BuildResult::Canceled) => NotificationLevel::Error,
        _ => NotificationLevel::Info,
    };
    tracing::info!(
        definition = build.definition.name,
        build_id = build.id,
        result = result_label,
        "pipeline completed"
    );
    app.notifications.push(level, message);
}
