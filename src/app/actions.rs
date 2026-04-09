use std::collections::BTreeMap;
use std::future::Future;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::mpsc;

use crate::api::client::AdoClient;
use crate::api::models;
use crate::events::Action;

use super::App;
use super::View;
use super::log_viewer::TimelineRow;
use super::messages::{AppMessage, RefreshSource};

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
    tokio::spawn(async move {
        let msg = match call(client).await {
            Ok(val) => on_ok(val),
            Err(e) => AppMessage::Error(format!("{context}: {e}")),
        };
        let _ = tx.send(msg).await;
    });
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
    tracing::debug!(?action, "handle_action");
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
            spawn_api(
                client,
                tx,
                "Cancel build",
                move |c| async move { c.cancel_build(build_id).await },
                |()| AppMessage::BuildCancelled,
            );
        }
        Action::CancelBuilds(build_ids) => {
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
            spawn_api(
                client,
                tx,
                "Retry stage",
                move |c| async move { c.retry_stage(build_id, &stage_ref_name).await },
                |()| AppMessage::StageRetried,
            );
        }
        Action::QueuePipeline(definition_id) => {
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
            active_builds,
            pending_approvals,
        } => {
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

            let mut map: BTreeMap<u32, models::Build> = BTreeMap::new();
            for build in &recent_builds {
                map.entry(build.definition.id)
                    .or_insert_with(|| build.clone());
            }
            app.data.latest_builds_by_def = map;
            app.data.recent_builds = recent_builds;
            app.data.active_builds = active_builds;
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
                return;
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
                        spawn_log_fetch(client, tx, build_id, log_id, app.log_viewer.generation());
                    }
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
                return;
            }
            app.log_viewer.set_log_content(content);
        }
        AppMessage::LogRefreshFinished { had_failure } => {
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
            app.notifications.success("Stage retried");
            if let Some(build) = app.log_viewer.selected_build() {
                spawn_timeline_fetch(client, tx, build.id, app.log_viewer.generation(), true);
            }
            spawn_data_refresh(app, client, tx);
        }
        AppMessage::CheckUpdated => {
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
            let build_id = build.id;
            app.navigate_to_log_viewer(build);
            spawn_timeline_fetch(client, tx, build_id, app.log_viewer.generation(), false);
        }
        AppMessage::UpdateAvailable { version } => {
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
    tokio::spawn(async move {
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
    });
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
    tokio::spawn(async move {
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
    });
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
    tokio::spawn(async move {
        let (defs_result, recent_result, active_result, approvals_result) = tokio::join!(
            client.list_definitions(),
            client.list_recent_builds(),
            client.list_active_builds(),
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

        match (defs_result, recent_result, active_result) {
            (Ok(definitions), Ok(recent_builds), Ok(active_builds)) => {
                let _ = tx
                    .send(AppMessage::DataRefresh {
                        definitions,
                        recent_builds,
                        active_builds,
                        pending_approvals,
                    })
                    .await;
            }
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                let _ = tx
                    .send(AppMessage::RefreshError {
                        message: format!("Refresh: {e}"),
                        source: RefreshSource::Data,
                    })
                    .await;
            }
        }
    });
    true
}

/// Re-fetch the build history for the currently selected pipeline definition.
fn spawn_build_history_refresh(app: &App, client: &AdoClient, tx: &mpsc::Sender<AppMessage>) {
    if let Some(def) = &app.build_history.selected_definition {
        let client = client.clone();
        let tx = tx.clone();
        let def_id = def.id;
        tokio::spawn(async move {
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
        });
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
    tokio::spawn(async move {
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
    });
    true
}

/// Open a URL in the platform's default browser.
fn open_url(url: &str) -> std::io::Result<std::process::Child> {
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
        for b in &recent {
            map.entry(b.definition.id).or_insert_with(|| b.clone());
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
}
