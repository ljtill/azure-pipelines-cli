//! Cross-cutting reducer logic for notifications, refresh errors, and update prompts.

use crate::shared::availability::Availability;
use crate::state::messages::RefreshSource;
use crate::state::notifications::NotificationLevel;
use crate::state::{App, CoreDataSnapshot, PaginationStatus};

use super::{
    DATA_REFRESH_BACKOFF_BASE_SECS, DATA_REFRESH_BACKOFF_MAX_SECS, LOG_REFRESH_BACKOFF_BASE_SECS,
    LOG_REFRESH_BACKOFF_MAX_SECS,
};

pub(in crate::state::actions) fn error(app: &mut App, message: String) {
    tracing::warn!(error = %message, "app error");
    app.notifications.error(message);
}

pub(in crate::state::actions) fn refresh_error(
    app: &mut App,
    message: String,
    source: RefreshSource,
) {
    tracing::warn!(error = %message, ?source, "refresh error");
    match source {
        RefreshSource::Data => {
            if app.refresh.data_refresh.in_flight {
                app.refresh.data_refresh.fail(
                    DATA_REFRESH_BACKOFF_BASE_SECS,
                    DATA_REFRESH_BACKOFF_MAX_SECS,
                );
            } else {
                tracing::debug!("ignoring data refresh error because no refresh is in flight");
            }
        }
        RefreshSource::Approvals => mark_approvals_refresh_error(app, &message),
        RefreshSource::BuildHistory | RefreshSource::Log => {}
    }
    app.notifications.error_dedup(message);
    // A refresh failure ends any in-flight paginated fetch too.
    app.refresh.pagination_status = None;
}

fn mark_approvals_refresh_error(app: &mut App, message: &str) {
    let previous_approvals = app.core.availability.pending_approvals.data().cloned();
    let had_previous_approvals =
        previous_approvals.is_some() || !app.core.data.pending_approvals.is_empty();
    let approvals = previous_approvals.unwrap_or_else(|| app.core.data.pending_approvals.clone());

    if had_previous_approvals {
        let definitions = app.core.data.definitions.clone();
        let recent_builds = app.core.data.recent_builds.clone();
        app.core
            .data
            .apply_refresh(definitions, recent_builds, approvals);
        app.core.availability.pending_approvals =
            Availability::stale(app.core.data.pending_approvals.clone(), message.to_string());
    } else {
        app.core.availability.pending_approvals = Availability::unavailable(message.to_string());
    }

    let snapshot = CoreDataSnapshot::from_data(&app.core.data, &app.core.retention_leases);
    let had_refresh_snapshot = app.core.availability.refresh.data().is_some()
        || snapshot.definitions > 0
        || snapshot.recent_builds > 0
        || snapshot.pending_approvals > 0
        || snapshot.retention_leases > 0;
    app.core.availability.refresh = if had_refresh_snapshot {
        Availability::stale(snapshot, message.to_string())
    } else {
        Availability::unavailable(message.to_string())
    };
}

pub(in crate::state::actions) fn refresh_cancelled(app: &mut App, source: RefreshSource) {
    tracing::debug!(?source, "refresh cancelled");
    match source {
        RefreshSource::Data => {
            app.refresh.data_refresh.cancel();
            app.refresh.pagination_status = None;
        }
        RefreshSource::Log => app.refresh.log_refresh.cancel(),
        RefreshSource::BuildHistory | RefreshSource::Approvals => {}
    }
}

pub(in crate::state::actions) fn update_available(app: &mut App, version: &str) {
    tracing::info!(version, "update available");
    app.notifications.push_persistent(
        NotificationLevel::Info,
        format!("Update available: v{version} — run 'devops update' to upgrade"),
    );
}

pub(in crate::state::actions) fn task_panicked(
    app: &mut App,
    task_name: &'static str,
    message: &str,
) {
    tracing::error!(task_name, %message, "background task panicked");
    match task_name {
        "data_refresh" if app.refresh.data_refresh.in_flight => {
            app.refresh.data_refresh.fail(
                DATA_REFRESH_BACKOFF_BASE_SECS,
                DATA_REFRESH_BACKOFF_MAX_SECS,
            );
            app.refresh.pagination_status = None;
        }
        "log_refresh" if app.refresh.log_refresh.in_flight => {
            app.refresh
                .log_refresh
                .fail(LOG_REFRESH_BACKOFF_BASE_SECS, LOG_REFRESH_BACKOFF_MAX_SECS);
        }
        _ => {}
    }
    app.notifications.push_persistent(
        NotificationLevel::Error,
        format!(
            "Background task '{task_name}' panicked: {message}. \
             Attach logs from ~/.local/state/devops/ when reporting."
        ),
    );
}

pub(in crate::state::actions) fn ado_api_version_unsupported(
    app: &mut App,
    requested: &str,
    server_message: &str,
) {
    tracing::error!(
        requested_api_version = %requested,
        server_message = %server_message,
        "Azure DevOps rejected requested api-version"
    );
    app.notifications.push_persistent(
        NotificationLevel::Error,
        format!(
            "Azure DevOps rejected api-version={requested}: {server_message}. \
             Pass --api-version or set DEVOPS_API_VERSION."
        ),
    );
}

pub(in crate::state::actions) fn pagination_progress(
    app: &mut App,
    endpoint: &'static str,
    page: usize,
    items: usize,
) {
    tracing::debug!(endpoint, page, items, "pagination progress");
    app.refresh.pagination_status = Some(PaginationStatus {
        endpoint,
        page,
        items,
    });
}
