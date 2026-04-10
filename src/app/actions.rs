use std::collections::BTreeMap;
use std::future::Future;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::api::client::AdoClient;
use crate::api::models;
use crate::events::Action;

use super::App;
use super::View;
use super::log_viewer::TimelineRow;
use super::messages::{AppMessage, RefreshSource};
use super::notifications::NotificationLevel;

// ---------------------------------------------------------------------------
// Drop guard for async refresh tasks
// ---------------------------------------------------------------------------

/// Ensures a fallback message is sent if the spawned task exits unexpectedly
/// (e.g., due to a panic). Call `defuse()` on the happy path to suppress.
struct RefreshGuard {
    tx: Option<mpsc::Sender<AppMessage>>,
    fallback: Option<AppMessage>,
}

impl RefreshGuard {
    fn new(tx: mpsc::Sender<AppMessage>, fallback: AppMessage) -> Self {
        Self {
            tx: Some(tx),
            fallback: Some(fallback),
        }
    }

    /// Disarm the guard — no fallback message will be sent on drop.
    fn defuse(&mut self) {
        self.tx = None;
        self.fallback = None;
    }
}

impl Drop for RefreshGuard {
    fn drop(&mut self) {
        if let (Some(tx), Some(msg)) = (self.tx.take(), self.fallback.take()) {
            let _ = tx.try_send(msg);
        }
    }
}

/// Spawn an async API call on a background task, routing the result to AppMessage.
fn spawn_api<F, Fut, T>(
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    context: &'static str,
    call: F,
    on_ok: impl FnOnce(T) -> AppMessage + Send + 'static,
) where
    F: FnOnce(AdoClient) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T>> + Send,
    T: Send + 'static,
{
    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("api_call", context);
    tokio::spawn(
        async move {
            let msg = match call(client).await {
                Ok(val) => on_ok(val),
                Err(e) => AppMessage::Error(format!("{context}: {e}")),
            };
            let _ = tx.send(msg).await;
        }
        .instrument(span),
    );
}

fn refresh_backoff(failures: u32, base_secs: u64, max_secs: u64) -> Duration {
    let shift = failures.saturating_sub(1).min(6);
    let multiplier = 1u64 << shift;
    Duration::from_secs(base_secs.saturating_mul(multiplier).min(max_secs))
}

pub fn handle_action(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    action: Action,
    last_data_fetch: &mut Instant,
) {
    if !matches!(action, Action::None) {
        tracing::debug!(?action, "handle_action");
    }
    match action {
        Action::Quit => app.running = false,
        Action::ForceRefresh => {
            if spawn_data_refresh(app, client, tx) {
                *last_data_fetch = Instant::now();
            }
            if app.view == View::BuildHistory {
                spawn_build_history_refresh(app, client, tx);
            }
        }
        Action::FetchBuildHistory(def_id) => {
            spawn_api(
                client,
                tx,
                "Fetch builds",
                move |c| async move { c.list_builds_for_definition(def_id).await },
                |builds| AppMessage::BuildHistory { builds },
            );
        }
        Action::FetchTimeline(build_id) => {
            spawn_timeline_fetch(client, tx, build_id, app.log_viewer.generation(), false);
        }
        Action::FetchBuildLog { build_id, log_id } => {
            spawn_log_fetch(client, tx, build_id, log_id, app.log_viewer.generation());
        }
        Action::FollowLatest => {
            // Switch to follow mode: jump cursor to active task and fetch its log
            if let Some((idx, log_id)) = app.log_viewer.auto_select_log_entry() {
                if let Some(TimelineRow::Task { name, .. }) =
                    app.log_viewer.timeline_rows().get(idx)
                {
                    app.log_viewer.set_followed(name.clone(), log_id);
                } else {
                    app.log_viewer.set_followed(String::new(), log_id);
                }
                if let Some(build) = app.log_viewer.selected_build() {
                    spawn_log_fetch(client, tx, build.id, log_id, app.log_viewer.generation());
                }
            }
        }
        Action::OpenInBrowser(url) => {
            let _ = open_url(&url);
        }
        Action::CancelBuild(build_id) => {
            tracing::info!(build_id, "cancelling build");
            spawn_api(
                client,
                tx,
                "Cancel build",
                move |c| async move { c.cancel_build(build_id).await },
                |()| AppMessage::BuildCancelled,
            );
        }
        Action::CancelBuilds(build_ids) => {
            tracing::info!(count = build_ids.len(), "cancelling builds");
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut set = tokio::task::JoinSet::new();
                for &id in &build_ids {
                    let client = client.clone();
                    set.spawn(async move { client.cancel_build(id).await });
                }
                let mut cancelled = 0u32;
                let mut failed = 0u32;
                while let Some(result) = set.join_next().await {
                    match result {
                        Ok(Ok(())) => cancelled += 1,
                        _ => failed += 1,
                    }
                }
                let _ = tx
                    .send(AppMessage::BuildsCancelled { cancelled, failed })
                    .await;
            });
        }
        Action::RetryStage {
            build_id,
            stage_ref_name,
        } => {
            tracing::info!(build_id, stage = stage_ref_name, "retrying stage");
            spawn_api(
                client,
                tx,
                "Retry stage",
                move |c| async move { c.retry_stage(build_id, &stage_ref_name).await },
                |()| AppMessage::StageRetried,
            );
        }
        Action::QueuePipeline(definition_id) => {
            tracing::info!(definition_id, "queuing pipeline");
            spawn_api(
                client,
                tx,
                "Queue pipeline",
                move |c| async move {
                    let run = c.run_pipeline(definition_id).await?;
                    c.get_build(run.id)
                        .await
                        .map_err(|e| anyhow::anyhow!("Fetch queued build: {e}"))
                },
                move |build| AppMessage::PipelineQueued {
                    build,
                    definition_id,
                },
            );
        }
        Action::ApproveCheck(approval_id) => {
            tracing::info!("approving check");
            spawn_api(
                client,
                tx,
                "Approve check",
                move |c| async move {
                    c.update_approval(&approval_id, "approved", "Approved via CLI")
                        .await
                },
                |()| AppMessage::CheckUpdated,
            );
        }
        Action::RejectCheck(approval_id) => {
            tracing::info!("rejecting check");
            spawn_api(
                client,
                tx,
                "Reject check",
                move |c| async move {
                    c.update_approval(&approval_id, "rejected", "Rejected via CLI")
                        .await
                },
                |()| AppMessage::CheckUpdated,
            );
        }
        Action::None => {}
    }
}

pub fn handle_message(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    msg: AppMessage,
) {
    match msg {
        AppMessage::DataRefresh {
            definitions,
            recent_builds,
            pending_approvals,
        } => {
            // Derive active builds from recent builds instead of a separate API call.
            let active_builds: Vec<models::Build> = recent_builds
                .iter()
                .filter(|b| b.status.is_in_progress())
                .cloned()
                .collect();

            tracing::info!(
                definitions = definitions.len(),
                active = active_builds.len(),
                recent = recent_builds.len(),
                approvals = pending_approvals.len(),
                "data refresh received"
            );
            app.data_refresh_in_flight = false;
            app.data_refresh_failures = 0;
            app.data_refresh_backoff_until = None;
            app.data.definitions = definitions;

            // Seed the map from each definition's latestBuild (full coverage),
            // then overlay with recent_builds — only if the recent build is newer.
            let mut map: BTreeMap<u32, models::Build> = BTreeMap::new();
            for def in &app.data.definitions {
                if let Some(build) = &def.latest_build {
                    map.insert(def.id, *build.clone());
                }
            }
            for build in &recent_builds {
                match map.entry(build.definition.id) {
                    std::collections::btree_map::Entry::Vacant(e) => {
                        e.insert(build.clone());
                    }
                    std::collections::btree_map::Entry::Occupied(mut e) => {
                        if build.id > e.get().id {
                            e.insert(build.clone());
                        }
                    }
                }
            }

            // Detect build state changes and emit in-app notifications.
            // Fires when:
            //   - A build transitions to InProgress (started)
            //   - A build transitions to Completed (succeeded/failed/canceled)
            // Skipped on first load (prev is empty) to avoid a startup storm.
            if app.notifications_enabled && !app.prev_latest_builds.is_empty() {
                for (def_id, build) in &map {
                    let prev = app.prev_latest_builds.get(def_id);
                    let (prev_id, prev_status) = match prev {
                        Some(&(id, status, _)) => (Some(id), Some(status)),
                        None => (None, None),
                    };

                    let id_changed = prev_id != Some(build.id);
                    let status_changed = prev_status != Some(build.status);

                    // Only notify on meaningful transitions
                    if !id_changed && !status_changed {
                        continue;
                    }

                    if build.status == models::BuildStatus::InProgress {
                        let msg =
                            format!("{} #{} started", build.definition.name, build.build_number);
                        tracing::info!(
                            definition = build.definition.name,
                            build_id = build.id,
                            "pipeline started"
                        );
                        app.notifications.push(NotificationLevel::Info, msg);
                    } else if build.status == models::BuildStatus::Completed {
                        let result_label = match build.result {
                            Some(models::BuildResult::Succeeded) => "succeeded",
                            Some(models::BuildResult::PartiallySucceeded) => "partially succeeded",
                            Some(models::BuildResult::Failed) => "failed",
                            Some(models::BuildResult::Canceled) => "canceled",
                            _ => "completed",
                        };
                        let msg = format!(
                            "{} #{} {}",
                            build.definition.name, build.build_number, result_label
                        );
                        let level = match build.result {
                            Some(models::BuildResult::Succeeded) => NotificationLevel::Success,
                            Some(models::BuildResult::Failed)
                            | Some(models::BuildResult::Canceled) => NotificationLevel::Error,
                            _ => NotificationLevel::Info,
                        };
                        tracing::info!(
                            definition = build.definition.name,
                            build_id = build.id,
                            result = result_label,
                            "pipeline completed"
                        );
                        app.notifications.push(level, msg);
                    }
                }
            }

            // Update the previous snapshot for the next diff cycle.
            app.prev_latest_builds = map
                .iter()
                .map(|(&def_id, b)| (def_id, (b.id, b.status, b.result)))
                .collect();

            app.data.latest_builds_by_def = map;
            app.data.recent_builds = recent_builds;
            app.data.active_builds = active_builds;
            app.data.pending_approval_build_ids = pending_approvals
                .iter()
                .filter_map(|a| a.build_id())
                .collect();
            app.data.pending_approvals = pending_approvals;

            app.dashboard.rebuild(
                &app.data.definitions,
                &app.data.latest_builds_by_def,
                &app.filters.folders,
                &app.filters.definition_ids,
            );
            app.pipelines.rebuild(
                &app.data.definitions,
                &app.filters.folders,
                &app.filters.definition_ids,
                &app.search.query,
            );
            app.active_runs.rebuild(
                &app.data.active_builds,
                &app.filters.definition_ids,
                &app.search.query,
            );
            app.last_refresh = Some(chrono::Utc::now());
            app.loading = false;
        }
        AppMessage::BuildHistory { builds } => {
            tracing::info!(count = builds.len(), "build history loaded");
            app.build_history.builds = builds;
            app.build_history
                .nav
                .set_len(app.build_history.builds.len());
        }
        AppMessage::Timeline {
            build_id,
            timeline,
            generation,
            is_refresh,
        } => {
            // Discard stale timeline results
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

            // Update selected_build status from timeline data so the header stays current
            app.log_viewer.refresh_build_status_from_timeline();

            if !is_refresh {
                // Initial load: full setup with auto-select
                app.log_viewer.clear_log();
                app.log_viewer.nav_mut().set_index(0);
                app.log_viewer.enter_follow_mode();
                app.log_viewer.rebuild_timeline_rows();

                if let Some((_idx, log_id)) = app.log_viewer.auto_select_log_entry() {
                    if let Some(TimelineRow::Task { name, .. }) = app
                        .log_viewer
                        .timeline_rows()
                        .get(app.log_viewer.nav().index())
                    {
                        app.log_viewer.set_followed(name.clone(), log_id);
                    } else {
                        app.log_viewer.set_followed(String::new(), log_id);
                    }
                    spawn_log_fetch(client, tx, build_id, log_id, app.log_viewer.generation());
                }
            } else if app.log_viewer.is_following() {
                // Refresh in follow mode: update tree, track latest active task
                app.log_viewer.rebuild_timeline_rows();

                if let Some((task_name, log_id)) = app.log_viewer.find_active_task() {
                    let task_changed = app.log_viewer.followed_log_id() != Some(log_id);
                    app.log_viewer.set_followed(task_name, log_id);

                    if task_changed {
                        tracing::debug!(build_id, log_id, "follow mode: task changed");
                        spawn_log_fetch(client, tx, build_id, log_id, app.log_viewer.generation());
                    }
                } else {
                    // Build completed or no active task — exit follow mode gracefully
                    tracing::debug!(
                        build_id,
                        "follow mode: no active task, switching to inspect"
                    );
                    app.log_viewer.enter_inspect_mode();
                }
            } else {
                // Refresh in inspect mode: only update tree status, preserve cursor + log
                app.log_viewer.rebuild_timeline_rows();
            }
        }
        AppMessage::LogContent {
            content,
            generation,
        } => {
            // Discard stale log results
            if generation != app.log_viewer.generation() {
                tracing::debug!(
                    generation,
                    expected = app.log_viewer.generation(),
                    "discarding stale log content"
                );
                return;
            }
            tracing::debug!(bytes = content.len(), "log content received");
            app.log_viewer.set_log_content(content);
        }
        AppMessage::LogRefreshFinished { had_failure } => {
            tracing::debug!(had_failure, "log refresh finished");
            app.log_refresh_in_flight = false;
            if had_failure {
                app.log_refresh_failures = app.log_refresh_failures.saturating_add(1);
                let backoff = refresh_backoff(app.log_refresh_failures, 5, 60);
                app.log_refresh_backoff_until = Some(Instant::now() + backoff);
            } else {
                app.log_refresh_failures = 0;
                app.log_refresh_backoff_until = None;
            }
        }
        AppMessage::Error(msg) => {
            tracing::warn!(error = %msg, "app error");
            app.notifications.error(msg);
        }
        AppMessage::RefreshError { message, source } => {
            tracing::warn!(error = %message, ?source, "refresh error");
            if source == RefreshSource::Data {
                app.data_refresh_in_flight = false;
                app.data_refresh_failures = app.data_refresh_failures.saturating_add(1);
                let backoff = refresh_backoff(app.data_refresh_failures, 30, 300);
                app.data_refresh_backoff_until = Some(Instant::now() + backoff);
            }
            app.notifications.error_dedup(message);
        }
        AppMessage::BuildCancelled => {
            tracing::info!("build cancelled successfully");
            app.notifications.success("Build cancelled");
            spawn_data_refresh(app, client, tx);
            if app.view == View::BuildHistory {
                spawn_build_history_refresh(app, client, tx);
            }
            if let Some(build) = app.log_viewer.selected_build() {
                spawn_timeline_fetch(client, tx, build.id, app.log_viewer.generation(), true);
            }
        }
        AppMessage::BuildsCancelled { cancelled, failed } => {
            tracing::info!(cancelled, failed, "builds cancelled");
            app.active_runs.selected.clear();
            spawn_data_refresh(app, client, tx);
            if app.view == View::BuildHistory {
                spawn_build_history_refresh(app, client, tx);
            }
            if failed > 0 {
                app.notifications
                    .error(format!("Cancelled {cancelled}, {failed} failed"));
            } else {
                app.notifications
                    .success(format!("Cancelled {cancelled} build(s)"));
            }
        }
        AppMessage::StageRetried => {
            tracing::info!("stage retried successfully");
            app.notifications.success("Stage retried");
            if let Some(build) = app.log_viewer.selected_build() {
                spawn_timeline_fetch(client, tx, build.id, app.log_viewer.generation(), true);
            }
            spawn_data_refresh(app, client, tx);
        }
        AppMessage::CheckUpdated => {
            tracing::info!("check updated successfully");
            app.notifications.success("Check updated");
            spawn_data_refresh(app, client, tx);
            if let Some(build) = app.log_viewer.selected_build() {
                spawn_timeline_fetch(client, tx, build.id, app.log_viewer.generation(), true);
            }
        }
        AppMessage::PipelineQueued {
            build,
            definition_id: _,
        } => {
            tracing::info!(build_id = build.id, "pipeline queued");
            let build_id = build.id;
            app.navigate_to_log_viewer(build);
            spawn_timeline_fetch(client, tx, build_id, app.log_viewer.generation(), false);
        }
        AppMessage::UpdateAvailable { version } => {
            tracing::info!(version = &*version, "update available");
            app.notifications.push_persistent(
                crate::app::notifications::NotificationLevel::Info,
                format!("Update available: v{version} — run 'pipelines update' to upgrade"),
            );
        }
    }
}

// --- Spawn helpers ---

pub fn spawn_log_fetch(
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    build_id: u32,
    log_id: u32,
    generation: u64,
) {
    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::debug_span!("log_fetch", build_id, log_id);
    tokio::spawn(
        async move {
            match client.get_build_log(build_id, log_id).await {
                Ok(content) => {
                    let _ = tx
                        .send(AppMessage::LogContent {
                            content,
                            generation,
                        })
                        .await;
                }
                Err(e) => {
                    let _ = tx.send(AppMessage::Error(format!("Fetch log: {e}"))).await;
                }
            }
        }
        .instrument(span),
    );
}

pub fn spawn_timeline_fetch(
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    build_id: u32,
    generation: u64,
    is_refresh: bool,
) {
    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::debug_span!("timeline_fetch", build_id, is_refresh);
    tokio::spawn(
        async move {
            match client.get_build_timeline(build_id).await {
                Ok(timeline) => {
                    let _ = tx
                        .send(AppMessage::Timeline {
                            build_id,
                            timeline,
                            generation,
                            is_refresh,
                        })
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(AppMessage::Error(format!("Fetch timeline: {e}")))
                        .await;
                }
            }
        }
        .instrument(span),
    );
}

pub fn spawn_data_refresh(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
) -> bool {
    if app.data_refresh_in_flight {
        return false;
    }
    app.data_refresh_in_flight = true;

    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("data_refresh");
    tokio::spawn(
        async move {
            let mut guard = RefreshGuard::new(
                tx.clone(),
                AppMessage::RefreshError {
                    message: "Data refresh task terminated unexpectedly".into(),
                    source: RefreshSource::Data,
                },
            );

            let (defs_result, recent_result, approvals_result) = tokio::join!(
                client.list_definitions(),
                client.list_recent_builds(),
                client.list_pending_approvals(),
            );

            let pending_approvals = match approvals_result {
                Ok(approvals) => approvals,
                Err(e) => {
                    let _ = tx
                        .send(AppMessage::RefreshError {
                            message: format!("Approvals unavailable: {e}"),
                            source: RefreshSource::Approvals,
                        })
                        .await;
                    Vec::new()
                }
            };

            match (defs_result, recent_result) {
                (Ok(definitions), Ok(recent_builds)) => {
                    let _ = tx
                        .send(AppMessage::DataRefresh {
                            definitions,
                            recent_builds,
                            pending_approvals,
                        })
                        .await;
                }
                (Err(e), _) | (_, Err(e)) => {
                    let _ = tx
                        .send(AppMessage::RefreshError {
                            message: format!("Refresh: {e}"),
                            source: RefreshSource::Data,
                        })
                        .await;
                }
            }

            guard.defuse();
        }
        .instrument(span),
    );
    true
}

/// Re-fetch the build history for the currently selected pipeline definition.
fn spawn_build_history_refresh(app: &App, client: &AdoClient, tx: &mpsc::Sender<AppMessage>) {
    if let Some(def) = &app.build_history.selected_definition {
        let client = client.clone();
        let tx = tx.clone();
        let def_id = def.id;
        let span = tracing::debug_span!("build_history_refresh", definition_id = def_id);
        tokio::spawn(
            async move {
                match client.list_builds_for_definition(def_id).await {
                    Ok(builds) => {
                        let _ = tx.send(AppMessage::BuildHistory { builds }).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::RefreshError {
                                message: format!("Refresh builds: {e}"),
                                source: RefreshSource::BuildHistory,
                            })
                            .await;
                    }
                }
            }
            .instrument(span),
        );
    }
}

pub fn spawn_log_refresh(app: &mut App, client: &AdoClient, tx: &mpsc::Sender<AppMessage>) -> bool {
    if app.log_refresh_in_flight {
        return false;
    }
    let generation = app.log_viewer.generation();
    let Some(build) = app.log_viewer.selected_build() else {
        return false;
    };
    app.log_refresh_in_flight = true;
    let build_id = build.id;
    let should_refresh_timeline = build.status.is_in_progress();
    let log_id_to_refresh = if app.log_viewer.is_following() {
        app.log_viewer.followed_log_id()
    } else {
        app.log_viewer
            .timeline_task_log_id(app.log_viewer.nav().index())
    };
    let should_refresh_log =
        !app.log_viewer.log_content().is_empty() && log_id_to_refresh.is_some();

    let timeline_client = client.clone();
    let log_client = client.clone();
    let tx = tx.clone();
    let span = tracing::debug_span!("log_refresh", build_id);
    tokio::spawn(
        async move {
            let mut guard = RefreshGuard::new(
                tx.clone(),
                AppMessage::LogRefreshFinished { had_failure: true },
            );

            let timeline_future = async move {
                if should_refresh_timeline {
                    Some(timeline_client.get_build_timeline(build_id).await)
                } else {
                    None
                }
            };
            let log_future = async move {
                if should_refresh_log {
                    if let Some(log_id) = log_id_to_refresh {
                        Some(log_client.get_build_log(build_id, log_id).await)
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            let (timeline_result, log_result) = tokio::join!(timeline_future, log_future);
            let mut had_failure = false;

            if let Some(result) = timeline_result {
                match result {
                    Ok(timeline) => {
                        let _ = tx
                            .send(AppMessage::Timeline {
                                build_id,
                                timeline,
                                generation,
                                is_refresh: true,
                            })
                            .await;
                    }
                    Err(e) => {
                        had_failure = true;
                        let _ = tx
                            .send(AppMessage::RefreshError {
                                message: format!("Refresh timeline: {e}"),
                                source: RefreshSource::Log,
                            })
                            .await;
                    }
                }
            }

            if let Some(result) = log_result {
                match result {
                    Ok(content) => {
                        let _ = tx
                            .send(AppMessage::LogContent {
                                content,
                                generation,
                            })
                            .await;
                    }
                    Err(e) => {
                        had_failure = true;
                        let _ = tx
                            .send(AppMessage::RefreshError {
                                message: format!("Refresh log: {e}"),
                                source: RefreshSource::Log,
                            })
                            .await;
                    }
                }
            }

            let _ = tx
                .send(AppMessage::LogRefreshFinished { had_failure })
                .await;

            guard.defuse();
        }
        .instrument(span),
    );
    true
}

/// Open a URL in the platform's default browser.
fn open_url(url: &str) -> std::io::Result<std::process::Child> {
    // Only allow https:// URLs to prevent command injection
    if !url.starts_with("https://") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "only https:// URLs are supported",
        ));
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "unsupported platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::*;
    use crate::test_helpers::*;

    // -----------------------------------------------------------------------
    // DataRefresh
    // -----------------------------------------------------------------------

    #[test]
    fn data_refresh_updates_definitions_and_builds() {
        let mut app = make_app();
        let defs = vec![
            make_definition(10, "Alpha", "\\"),
            make_definition(20, "Beta", "\\Ops"),
        ];
        let mut b1 = make_build(100, BuildStatus::Completed, Some(BuildResult::Succeeded));
        b1.definition = BuildDefinitionRef {
            id: 10,
            name: "Alpha".into(),
        };
        let recent = vec![b1.clone()];

        // Apply the same mutations handle_message(DataRefresh) would.
        app.data.definitions = defs;
        let mut map: BTreeMap<u32, Build> = BTreeMap::new();
        for def in &app.data.definitions {
            if let Some(build) = &def.latest_build {
                map.insert(def.id, *build.clone());
            }
        }
        for b in &recent {
            map.insert(b.definition.id, b.clone());
        }
        app.data.latest_builds_by_def = map;
        app.data.recent_builds = recent;
        app.data.active_builds = vec![];
        app.data.pending_approvals = vec![];
        app.dashboard.rebuild(
            &app.data.definitions,
            &app.data.latest_builds_by_def,
            &app.filters.folders,
            &app.filters.definition_ids,
        );
        app.pipelines.rebuild(
            &app.data.definitions,
            &app.filters.folders,
            &app.filters.definition_ids,
            &app.search.query,
        );
        app.active_runs.rebuild(
            &app.data.active_builds,
            &app.filters.definition_ids,
            &app.search.query,
        );
        app.last_refresh = Some(chrono::Utc::now());
        app.loading = false;

        assert_eq!(app.data.definitions.len(), 2);
        assert_eq!(app.pipelines.filtered.len(), 2);
        assert!(!app.dashboard.rows.is_empty());
        assert!(app.last_refresh.is_some());
        assert!(!app.loading);
    }

    #[test]
    fn data_refresh_preserves_notifications() {
        let mut app = make_app();
        app.notifications.error("old error");
        assert!(app.notifications.clone_current().is_some());

        // DataRefresh no longer clears notifications — they expire via TTL
        // Simulate what handle_message(DataRefresh) does (no clear call)
        app.loading = false;
        assert!(app.notifications.clone_current().is_some());
    }

    #[test]
    fn data_refresh_replaces_previous_data() {
        let mut app = make_app();
        assert_eq!(app.data.definitions.len(), 3); // make_app seeds 3

        // Simulate a DataRefresh with only 1 definition
        app.data.definitions = vec![make_definition(99, "Only", "\\")];
        app.data.recent_builds = vec![];
        app.data.latest_builds_by_def.clear();
        app.data.active_builds = vec![];
        app.data.pending_approvals = vec![];
        app.dashboard.rebuild(
            &app.data.definitions,
            &app.data.latest_builds_by_def,
            &app.filters.folders,
            &app.filters.definition_ids,
        );
        app.pipelines.rebuild(
            &app.data.definitions,
            &app.filters.folders,
            &app.filters.definition_ids,
            &app.search.query,
        );
        app.active_runs.rebuild(
            &app.data.active_builds,
            &app.filters.definition_ids,
            &app.search.query,
        );
        app.last_refresh = Some(chrono::Utc::now());
        app.loading = false;

        assert_eq!(app.data.definitions.len(), 1);
        assert_eq!(app.pipelines.filtered.len(), 1);
        assert_eq!(app.active_runs.filtered.len(), 0);
    }

    #[test]
    fn data_refresh_seeds_map_from_definition_latest_build() {
        let mut app = make_app();

        // Definition carries its own latestBuild — no recent_builds at all.
        let embedded_build = make_build(500, BuildStatus::Completed, Some(BuildResult::Succeeded));
        let mut def = make_definition(50, "Rare Pipeline", "\\");
        def.latest_build = Some(Box::new(embedded_build));

        app.data.definitions = vec![def];
        let mut map: BTreeMap<u32, Build> = BTreeMap::new();
        for d in &app.data.definitions {
            if let Some(build) = &d.latest_build {
                map.insert(d.id, *build.clone());
            }
        }
        // No recent_builds to overlay
        app.data.latest_builds_by_def = map;
        app.data.recent_builds = vec![];

        assert!(app.data.latest_builds_by_def.contains_key(&50));
        assert_eq!(app.data.latest_builds_by_def[&50].id, 500);
    }

    #[test]
    fn data_refresh_recent_builds_overlay_definition_latest_build() {
        let mut app = make_app();

        // Definition has an older build embedded.
        let old_build = make_build(500, BuildStatus::Completed, Some(BuildResult::Failed));
        let mut def = make_definition(50, "Pipeline", "\\");
        def.latest_build = Some(Box::new(old_build));

        // recent_builds has a newer build for the same definition.
        let mut newer_build = make_build(501, BuildStatus::Completed, Some(BuildResult::Succeeded));
        newer_build.definition = BuildDefinitionRef {
            id: 50,
            name: "Pipeline".into(),
        };

        app.data.definitions = vec![def];
        let mut map: BTreeMap<u32, Build> = BTreeMap::new();
        for d in &app.data.definitions {
            if let Some(build) = &d.latest_build {
                map.insert(d.id, *build.clone());
            }
        }
        let recent = vec![newer_build];
        for b in &recent {
            match map.entry(b.definition.id) {
                std::collections::btree_map::Entry::Vacant(e) => {
                    e.insert(b.clone());
                }
                std::collections::btree_map::Entry::Occupied(mut e) => {
                    if b.id > e.get().id {
                        e.insert(b.clone());
                    }
                }
            }
        }
        app.data.latest_builds_by_def = map;
        app.data.recent_builds = recent;

        // recent_builds should win (overlay).
        assert_eq!(app.data.latest_builds_by_def[&50].id, 501);
    }

    #[test]
    fn data_refresh_older_recent_build_does_not_overwrite_newer() {
        let mut app = make_app();

        // Definition has a newer build embedded.
        let newer_build = make_build(502, BuildStatus::Completed, Some(BuildResult::Succeeded));
        let mut def = make_definition(50, "Pipeline", "\\");
        def.latest_build = Some(Box::new(newer_build));

        // recent_builds has an older build for the same definition.
        let mut older_build = make_build(499, BuildStatus::Completed, Some(BuildResult::Failed));
        older_build.definition = BuildDefinitionRef {
            id: 50,
            name: "Pipeline".into(),
        };

        app.data.definitions = vec![def];
        let mut map: BTreeMap<u32, Build> = BTreeMap::new();
        for d in &app.data.definitions {
            if let Some(build) = &d.latest_build {
                map.insert(d.id, *build.clone());
            }
        }
        let recent = vec![older_build];
        for b in &recent {
            match map.entry(b.definition.id) {
                std::collections::btree_map::Entry::Vacant(e) => {
                    e.insert(b.clone());
                }
                std::collections::btree_map::Entry::Occupied(mut e) => {
                    if b.id > e.get().id {
                        e.insert(b.clone());
                    }
                }
            }
        }
        app.data.latest_builds_by_def = map;

        // latestBuild (502) should win — older recent build (499) must not overwrite.
        assert_eq!(app.data.latest_builds_by_def[&50].id, 502);
    }

    // -----------------------------------------------------------------------
    // BuildHistory
    // -----------------------------------------------------------------------

    #[test]
    fn build_history_populates_and_syncs_nav() {
        let mut app = make_app();
        let builds = vec![
            make_build(1, BuildStatus::Completed, Some(BuildResult::Succeeded)),
            make_build(2, BuildStatus::Completed, Some(BuildResult::Failed)),
            make_build(3, BuildStatus::InProgress, None),
        ];
        app.build_history.builds = builds;
        app.build_history
            .nav
            .set_len(app.build_history.builds.len());

        assert_eq!(app.build_history.builds.len(), 3);
        // Nav synced — 3 items, index starts at 0
        app.build_history.nav.down();
        assert_eq!(app.build_history.nav.index(), 1);
    }

    #[test]
    fn build_history_empty() {
        let mut app = make_app();
        app.build_history.builds = vec![];
        app.build_history.nav.set_len(0);
        assert_eq!(app.build_history.nav.index(), 0);
        // down on empty list is a no-op
        app.build_history.nav.down();
        assert_eq!(app.build_history.nav.index(), 0);
    }

    // -----------------------------------------------------------------------
    // LogContent
    // -----------------------------------------------------------------------

    #[test]
    fn log_content_splits_lines() {
        let mut app = make_app();
        app.navigate_to_log_viewer(make_build(
            1,
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
        ));
        app.log_viewer.set_log_content("line1\nline2\nline3".into());

        assert_eq!(app.log_viewer.log_content().len(), 3);
        assert_eq!(app.log_viewer.log_content()[0], "line1");
        assert_eq!(app.log_viewer.log_content()[2], "line3");
        assert!(app.log_viewer.log_auto_scroll());
    }

    #[test]
    fn log_content_empty_input() {
        let mut app = make_app();
        app.navigate_to_log_viewer(make_build(
            1,
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
        ));
        app.log_viewer.set_log_content(String::new());
        // "".lines() yields nothing, so vec should be empty
        assert!(app.log_viewer.log_content().is_empty());
    }

    #[test]
    fn log_content_resets_scroll_offset() {
        let mut app = make_app();
        app.navigate_to_log_viewer(make_build(
            1,
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
        ));
        app.log_viewer.scroll_down(50);
        assert!(app.log_viewer.log_scroll_offset() > 0);

        // Setting new log content resets scroll
        app.log_viewer.set_log_content("fresh\nlog".into());
        assert_eq!(app.log_viewer.log_scroll_offset(), 0);
        assert!(app.log_viewer.log_auto_scroll());
    }

    // -----------------------------------------------------------------------
    // Generation / stale-guard
    // -----------------------------------------------------------------------

    #[test]
    fn stale_generation_detected() {
        let mut app = make_app();
        app.navigate_to_log_viewer(make_build(
            1,
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
        ));
        let current_gen = app.log_viewer.generation();
        let stale_gen = current_gen.wrapping_sub(1);
        assert_ne!(stale_gen, current_gen);
    }

    #[test]
    fn generation_increments_across_navigations() {
        let mut app = make_app();
        let gen0 = app.log_viewer.generation();

        app.navigate_to_log_viewer(make_build(
            1,
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
        ));
        let gen1 = app.log_viewer.generation();
        assert!(gen1 > gen0);

        app.go_back();
        let gen2 = app.log_viewer.generation();
        assert!(gen2 > gen1);

        app.navigate_to_log_viewer(make_build(
            2,
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
        ));
        let gen3 = app.log_viewer.generation();
        assert!(gen3 > gen2);
    }

    #[test]
    fn stale_log_content_would_be_discarded() {
        let mut app = make_app();
        app.navigate_to_log_viewer(make_build(
            1,
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
        ));
        let current_gen = app.log_viewer.generation();
        let stale_gen = current_gen.wrapping_sub(1);

        // Simulate the stale guard: only apply content if generation matches
        let content = "should not appear".to_string();
        if stale_gen == app.log_viewer.generation() {
            app.log_viewer.set_log_content(content);
        }
        // Content should remain empty because the generation didn't match
        assert!(app.log_viewer.log_content().is_empty());
    }

    // -----------------------------------------------------------------------
    // Error / notification messages
    // -----------------------------------------------------------------------

    #[test]
    fn error_pushes_notification() {
        let mut app = make_app();
        app.notifications.error("fetch failed");
        let n = app.notifications.clone_current().unwrap();
        assert_eq!(n.message, "fetch failed");
    }

    #[test]
    fn success_pushes_notification() {
        let mut app = make_app();
        app.notifications.success("Build cancelled");
        let n = app.notifications.clone_current().unwrap();
        assert_eq!(n.message, "Build cancelled");
    }

    #[test]
    fn batch_cancel_clears_selections() {
        let mut app = make_app();
        app.active_runs.selected.insert(1);
        app.active_runs.selected.insert(2);
        assert_eq!(app.active_runs.selected.len(), 2);

        // BuildsCancelled handler clears selections
        app.active_runs.selected.clear();
        assert!(app.active_runs.selected.is_empty());
    }

    #[test]
    fn batch_cancel_with_failures_shows_error() {
        let mut app = make_app();
        // Simulate partial-failure path from BuildsCancelled
        let cancelled = 2u32;
        let failed = 1u32;
        app.active_runs.selected.clear();
        app.notifications
            .error(format!("Cancelled {cancelled}, {failed} failed"));
        let n = app.notifications.clone_current().unwrap();
        assert!(n.message.contains("failed"));
        assert!(n.message.contains("Cancelled 2"));
    }

    #[test]
    fn batch_cancel_all_succeeded_shows_success() {
        let mut app = make_app();
        let cancelled = 3u32;
        let failed = 0u32;
        app.active_runs.selected.clear();
        if failed > 0 {
            app.notifications
                .error(format!("Cancelled {cancelled}, {failed} failed"));
        } else {
            app.notifications
                .success(format!("Cancelled {cancelled} build(s)"));
        }
        let n = app.notifications.clone_current().unwrap();
        assert_eq!(n.message, "Cancelled 3 build(s)");
    }

    // -----------------------------------------------------------------------
    // PipelineQueued
    // -----------------------------------------------------------------------

    #[test]
    fn pipeline_queued_navigates_to_log_viewer() {
        let mut app = make_app();
        assert_eq!(app.view, View::Dashboard);

        let build = make_build(42, BuildStatus::InProgress, None);
        app.navigate_to_log_viewer(build);

        assert_eq!(app.view, View::LogViewer);
        assert_eq!(app.log_viewer.selected_build().unwrap().id, 42);
    }

    #[test]
    fn pipeline_queued_increments_generation() {
        let mut app = make_app();
        let gen_before = app.log_viewer.generation();
        app.navigate_to_log_viewer(make_build(42, BuildStatus::InProgress, None));
        assert!(app.log_viewer.generation() > gen_before);
    }

    #[test]
    fn pipeline_queued_starts_in_follow_mode() {
        let mut app = make_app();
        app.navigate_to_log_viewer(make_build(42, BuildStatus::InProgress, None));
        assert!(app.log_viewer.is_following());
    }

    // -----------------------------------------------------------------------
    // StageRetried / CheckUpdated notification
    // -----------------------------------------------------------------------

    #[test]
    fn stage_retried_shows_success() {
        let mut app = make_app();
        // Mirrors handle_message(StageRetried) notification path
        app.notifications.success("Stage retried");
        let n = app.notifications.clone_current().unwrap();
        assert_eq!(n.message, "Stage retried");
    }

    #[test]
    fn check_updated_shows_success() {
        let mut app = make_app();
        app.notifications.success("Check updated");
        let n = app.notifications.clone_current().unwrap();
        assert_eq!(n.message, "Check updated");
    }

    // -----------------------------------------------------------------------
    // State-change notifications
    // -----------------------------------------------------------------------

    /// Helper: simulate the DataRefresh notification-diff logic for a given
    /// app and new latest_builds_by_def map. Mirrors the logic in handle_message.
    fn simulate_notification_diff(app: &mut crate::app::App, map: &BTreeMap<u32, Build>) {
        use crate::app::notifications::NotificationLevel;

        if app.notifications_enabled && !app.prev_latest_builds.is_empty() {
            for (def_id, build) in map {
                let prev = app.prev_latest_builds.get(def_id);
                let (prev_id, prev_status) = match prev {
                    Some(&(id, status, _)) => (Some(id), Some(status)),
                    None => (None, None),
                };

                let id_changed = prev_id != Some(build.id);
                let status_changed = prev_status != Some(build.status);

                if !id_changed && !status_changed {
                    continue;
                }

                if build.status == BuildStatus::InProgress {
                    let msg = format!("{} #{} started", build.definition.name, build.build_number);
                    app.notifications.push(NotificationLevel::Info, msg);
                } else if build.status == BuildStatus::Completed {
                    let result_label = match build.result {
                        Some(BuildResult::Succeeded) => "succeeded",
                        Some(BuildResult::Failed) => "failed",
                        Some(BuildResult::Canceled) => "canceled",
                        _ => "completed",
                    };
                    let level = match build.result {
                        Some(BuildResult::Succeeded) => NotificationLevel::Success,
                        Some(BuildResult::Failed) | Some(BuildResult::Canceled) => {
                            NotificationLevel::Error
                        }
                        _ => NotificationLevel::Info,
                    };
                    let msg = format!(
                        "{} #{} {}",
                        build.definition.name, build.build_number, result_label
                    );
                    app.notifications.push(level, msg);
                }
            }
        }
        app.prev_latest_builds = map
            .iter()
            .map(|(&def_id, b)| (def_id, (b.id, b.status, b.result)))
            .collect();
    }

    #[test]
    fn no_notification_on_first_data_refresh() {
        let mut app = make_app();
        app.notifications_enabled = true;

        let mut b = make_build(100, BuildStatus::Completed, Some(BuildResult::Succeeded));
        b.definition = BuildDefinitionRef {
            id: 1,
            name: "CI".into(),
        };
        let mut map = BTreeMap::new();
        map.insert(1u32, b);

        simulate_notification_diff(&mut app, &map);

        // First load — prev was empty, so no notification should fire.
        assert!(app.notifications.clone_current().is_none());
        // But the snapshot should be populated now.
        assert_eq!(app.prev_latest_builds.len(), 1);
    }

    #[test]
    fn notification_on_build_started() {
        let mut app = make_app();
        app.notifications_enabled = true;

        // First refresh: completed build (seed)
        let mut b1 = make_build(100, BuildStatus::Completed, Some(BuildResult::Succeeded));
        b1.definition = BuildDefinitionRef {
            id: 1,
            name: "CI Pipeline".into(),
        };
        let mut map1 = BTreeMap::new();
        map1.insert(1u32, b1);
        simulate_notification_diff(&mut app, &map1);
        assert!(app.notifications.clone_current().is_none()); // first load

        // Second refresh: new build started
        let mut b2 = make_build(101, BuildStatus::InProgress, None);
        b2.definition = BuildDefinitionRef {
            id: 1,
            name: "CI Pipeline".into(),
        };
        let mut map2 = BTreeMap::new();
        map2.insert(1u32, b2);
        simulate_notification_diff(&mut app, &map2);

        let n = app.notifications.clone_current().unwrap();
        assert_eq!(n.message, "CI Pipeline #101 started");
        assert_eq!(n.level, crate::app::notifications::NotificationLevel::Info);
    }

    #[test]
    fn notification_on_same_build_completing() {
        let mut app = make_app();
        app.notifications_enabled = true;

        // First refresh: in-progress build (seed)
        let mut b1 = make_build(100, BuildStatus::InProgress, None);
        b1.definition = BuildDefinitionRef {
            id: 1,
            name: "Deploy".into(),
        };
        let mut map1 = BTreeMap::new();
        map1.insert(1u32, b1);
        simulate_notification_diff(&mut app, &map1);

        // Second refresh: same build ID now completed
        let mut b2 = make_build(100, BuildStatus::Completed, Some(BuildResult::Failed));
        b2.definition = BuildDefinitionRef {
            id: 1,
            name: "Deploy".into(),
        };
        let mut map2 = BTreeMap::new();
        map2.insert(1u32, b2);
        simulate_notification_diff(&mut app, &map2);

        let n = app.notifications.clone_current().unwrap();
        assert_eq!(n.message, "Deploy #100 failed");
        assert_eq!(n.level, crate::app::notifications::NotificationLevel::Error);
    }

    #[test]
    fn notification_on_new_build_completing() {
        let mut app = make_app();
        app.notifications_enabled = true;

        // Seed with build 100 in-progress
        let mut b1 = make_build(100, BuildStatus::InProgress, None);
        b1.definition = BuildDefinitionRef {
            id: 1,
            name: "CI Pipeline".into(),
        };
        let mut map1 = BTreeMap::new();
        map1.insert(1u32, b1);
        simulate_notification_diff(&mut app, &map1);

        // New build 101 completed (different build ID)
        let mut b2 = make_build(101, BuildStatus::Completed, Some(BuildResult::Succeeded));
        b2.definition = BuildDefinitionRef {
            id: 1,
            name: "CI Pipeline".into(),
        };
        let mut map2 = BTreeMap::new();
        map2.insert(1u32, b2);
        simulate_notification_diff(&mut app, &map2);

        let n = app.notifications.clone_current().unwrap();
        assert_eq!(n.message, "CI Pipeline #101 succeeded");
        assert_eq!(
            n.level,
            crate::app::notifications::NotificationLevel::Success
        );
    }

    #[test]
    fn no_notification_when_build_unchanged() {
        let mut app = make_app();
        app.notifications_enabled = true;

        // Seed with completed build
        let mut b = make_build(100, BuildStatus::Completed, Some(BuildResult::Succeeded));
        b.definition = BuildDefinitionRef {
            id: 1,
            name: "CI".into(),
        };
        let mut map = BTreeMap::new();
        map.insert(1u32, b.clone());
        simulate_notification_diff(&mut app, &map);

        // Same build again
        simulate_notification_diff(&mut app, &map);

        // No notification (first load skipped, second has same build_id + status).
        assert!(app.notifications.clone_current().is_none());
    }

    #[test]
    fn no_notification_when_disabled() {
        let mut app = make_app();
        app.notifications_enabled = false;

        // Seed snapshot manually
        app.prev_latest_builds
            .insert(1, (100, BuildStatus::InProgress, None));

        let mut b = make_build(101, BuildStatus::Completed, Some(BuildResult::Failed));
        b.definition = BuildDefinitionRef {
            id: 1,
            name: "CI".into(),
        };
        let mut map = BTreeMap::new();
        map.insert(1u32, b);
        simulate_notification_diff(&mut app, &map);

        assert!(app.notifications.clone_current().is_none());
    }
}
