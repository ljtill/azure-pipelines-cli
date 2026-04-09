use std::collections::BTreeMap;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::api::client::AdoClient;
use crate::api::models;
use crate::events::Action;

use super::App;
use super::messages::AppMessage;
use super::timeline::TimelineRow;

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
            spawn_data_refresh(client, tx);
            *last_data_fetch = Instant::now();
        }
        Action::FetchBuildHistory(def_id) => {
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                match client.list_builds_for_definition(def_id).await {
                    Ok(builds) => {
                        let _ = tx.send(AppMessage::BuildHistory { builds }).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::Error(format!("Fetch builds: {e}")))
                            .await;
                    }
                }
            });
        }
        Action::FetchTimeline(build_id) => {
            spawn_timeline_fetch(client, tx, build_id, app.log_viewer.log_generation, false);
        }
        Action::FetchBuildLog { build_id, log_id } => {
            spawn_log_fetch(client, tx, build_id, log_id, app.log_viewer.log_generation);
        }
        Action::FollowLatest => {
            // Switch to follow mode: jump cursor to active task and fetch its log
            if let Some((idx, log_id)) = app.auto_select_log_entry() {
                if let Some(TimelineRow::Task { name, .. }) = app.log_viewer.timeline_rows.get(idx)
                {
                    app.log_viewer.followed_task_name = name.clone();
                }
                app.log_viewer.followed_log_id = Some(log_id);
                if let Some(build) = &app.log_viewer.selected_build {
                    spawn_log_fetch(client, tx, build.id, log_id, app.log_viewer.log_generation);
                }
            }
        }
        Action::OpenInBrowser(url) => {
            let _ = open_url(&url);
        }
        Action::CancelBuild(build_id) => {
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                match client.cancel_build(build_id).await {
                    Ok(()) => {
                        let _ = tx.send(AppMessage::BuildCancelled).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::Error(format!("Cancel build: {e}")))
                            .await;
                    }
                }
            });
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
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                match client.retry_stage(build_id, &stage_ref_name).await {
                    Ok(()) => {
                        let _ = tx.send(AppMessage::StageRetried).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::Error(format!("Retry stage: {e}")))
                            .await;
                    }
                }
            });
        }
        Action::QueuePipeline(definition_id) => {
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                match client.run_pipeline(definition_id).await {
                    Ok(run) => match client.get_build(run.id).await {
                        Ok(build) => {
                            let _ = tx
                                .send(AppMessage::PipelineQueued {
                                    build,
                                    definition_id,
                                })
                                .await;
                        }
                        Err(e) => {
                            let _ = tx
                                .send(AppMessage::Error(format!("Fetch queued build: {e}")))
                                .await;
                        }
                    },
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::Error(format!("Queue pipeline: {e}")))
                            .await;
                    }
                }
            });
        }
        Action::ApproveCheck(approval_id) => {
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                match client
                    .update_approval(&approval_id, "approved", "Approved via CLI")
                    .await
                {
                    Ok(()) => {
                        let _ = tx.send(AppMessage::CheckUpdated).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::Error(format!("Approve check: {e}")))
                            .await;
                    }
                }
            });
        }
        Action::RejectCheck(approval_id) => {
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                match client
                    .update_approval(&approval_id, "rejected", "Rejected via CLI")
                    .await
                {
                    Ok(()) => {
                        let _ = tx.send(AppMessage::CheckUpdated).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::Error(format!("Reject check: {e}")))
                            .await;
                    }
                }
            });
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
            app.definitions = definitions;

            let mut map: BTreeMap<u32, models::Build> = BTreeMap::new();
            for build in &recent_builds {
                map.entry(build.definition.id)
                    .or_insert_with(|| build.clone());
            }
            app.latest_builds_by_def = map;
            app.recent_builds = recent_builds;
            app.active_builds = active_builds;
            app.pending_approvals = pending_approvals;

            app.rebuild_dashboard_rows();
            app.rebuild_filtered_pipelines();
            app.rebuild_filtered_active_builds();
            app.last_refresh = Some(chrono::Utc::now());
            app.loading = false;
            app.error_message = None;
        }
        AppMessage::BuildHistory { builds } => {
            app.definition_builds = builds;
        }
        AppMessage::Timeline {
            build_id,
            timeline,
            generation,
            is_refresh,
        } => {
            // Discard stale timeline results
            if generation != app.log_viewer.log_generation {
                return;
            }

            app.log_viewer.build_timeline = Some(timeline);

            // Update selected_build status from timeline data so the header stays current
            app.refresh_build_status_from_timeline();

            if !is_refresh {
                // Initial load: full setup with auto-select
                app.log_viewer.log_content.clear();
                app.log_viewer.log_entries_index = 0;
                app.log_viewer.follow_mode = true;
                app.rebuild_timeline_rows();

                if let Some((_idx, log_id)) = app.auto_select_log_entry() {
                    if let Some(TimelineRow::Task { name, .. }) = app
                        .log_viewer
                        .timeline_rows
                        .get(app.log_viewer.log_entries_index)
                    {
                        app.log_viewer.followed_task_name = name.clone();
                    }
                    app.log_viewer.followed_log_id = Some(log_id);
                    spawn_log_fetch(client, tx, build_id, log_id, app.log_viewer.log_generation);
                }
            } else if app.log_viewer.follow_mode {
                // Refresh in follow mode: update tree, track latest active task
                app.rebuild_timeline_rows();

                if let Some((task_name, log_id)) = app.find_active_task() {
                    let task_changed = app.log_viewer.followed_log_id != Some(log_id);
                    app.log_viewer.followed_task_name = task_name;
                    app.log_viewer.followed_log_id = Some(log_id);

                    if task_changed {
                        spawn_log_fetch(
                            client,
                            tx,
                            build_id,
                            log_id,
                            app.log_viewer.log_generation,
                        );
                    }
                }
            } else {
                // Refresh in inspect mode: only update tree status, preserve cursor + log
                app.rebuild_timeline_rows();
            }
        }
        AppMessage::LogContent {
            content,
            generation,
        } => {
            // Discard stale log results
            if generation != app.log_viewer.log_generation {
                return;
            }
            app.log_viewer.log_content = content.lines().map(String::from).collect();
            app.log_viewer.log_auto_scroll = true;
            app.log_viewer.log_scroll_offset = 0;
        }
        AppMessage::Error(msg) => {
            tracing::warn!(error = %msg, "app error");
            app.error_message = Some(msg);
        }
        AppMessage::BuildCancelled => {
            spawn_data_refresh(client, tx);
            if let Some(build) = &app.log_viewer.selected_build {
                spawn_timeline_fetch(client, tx, build.id, app.log_viewer.log_generation, true);
            }
        }
        AppMessage::BuildsCancelled { cancelled, failed } => {
            app.selected_builds.clear();
            spawn_data_refresh(client, tx);
            if failed > 0 {
                app.error_message = Some(format!("Cancelled {cancelled}, {failed} failed"));
            }
        }
        AppMessage::StageRetried => {
            if let Some(build) = &app.log_viewer.selected_build {
                spawn_timeline_fetch(client, tx, build.id, app.log_viewer.log_generation, true);
            }
            spawn_data_refresh(client, tx);
        }
        AppMessage::CheckUpdated => {
            spawn_data_refresh(client, tx);
            if let Some(build) = &app.log_viewer.selected_build {
                spawn_timeline_fetch(client, tx, build.id, app.log_viewer.log_generation, true);
            }
        }
        AppMessage::PipelineQueued {
            build,
            definition_id: _,
        } => {
            let build_id = build.id;
            app.navigate_to_log_viewer(build);
            spawn_timeline_fetch(client, tx, build_id, app.log_viewer.log_generation, false);
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

pub fn spawn_data_refresh(client: &AdoClient, tx: &mpsc::Sender<AppMessage>) {
    let client = client.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        let (defs_result, recent_result, active_result, approvals_result) = tokio::join!(
            client.list_definitions(),
            client.list_recent_builds(),
            client.list_active_builds(),
            client.list_pending_approvals(),
        );

        let pending_approvals = approvals_result.unwrap_or_default();

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
                let _ = tx.send(AppMessage::Error(format!("Refresh: {e}"))).await;
            }
        }
    });
}

pub fn spawn_log_refresh(app: &App, client: &AdoClient, tx: &mpsc::Sender<AppMessage>) {
    let generation = app.log_viewer.log_generation;

    // Re-fetch timeline for in-progress builds
    if let Some(build) = &app.log_viewer.selected_build
        && build.status.is_in_progress()
    {
        spawn_timeline_fetch(client, tx, build.id, generation, true);
    }

    // Re-fetch log content for the currently viewed task.
    // In follow mode: refresh the followed task's log.
    // In inspect mode: refresh the selected (pinned) task's log.
    let log_id_to_refresh = if app.log_viewer.follow_mode {
        app.log_viewer.followed_log_id
    } else {
        app.timeline_task_log_id(app.log_viewer.log_entries_index)
    };

    if !app.log_viewer.log_content.is_empty()
        && let Some(build) = &app.log_viewer.selected_build
        && let Some(log_id) = log_id_to_refresh
    {
        let client = client.clone();
        let tx = tx.clone();
        let build_id = build.id;
        tokio::spawn(async move {
            if let Ok(content) = client.get_build_log(build_id, log_id).await {
                let _ = tx
                    .send(AppMessage::LogContent {
                        content,
                        generation,
                    })
                    .await;
            }
        });
    }
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
