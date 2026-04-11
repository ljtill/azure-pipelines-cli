//! Dispatches user actions to state mutations and async tasks.

use std::time::Instant;

use tokio::sync::mpsc;

use crate::client::http::AdoClient;
use crate::events::Action;

use super::super::App;
use super::super::TimelineRow;
use super::super::View;
use super::super::messages::AppMessage;
use super::spawn::{
    open_url, spawn_api, spawn_build_history_refresh, spawn_data_refresh, spawn_log_fetch,
    spawn_timeline_fetch,
};

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
        Action::Reload => app.running = false,
        Action::ForceRefresh => {
            if spawn_data_refresh(app, client, tx) {
                *last_data_fetch = Instant::now();
            }
            if app.view == View::BuildHistory {
                spawn_build_history_refresh(app, client, tx, None);
            }
        }
        Action::FetchBuildHistory(def_id) => {
            spawn_api(
                client,
                tx,
                "Fetch builds",
                move |c| async move { c.list_builds_for_definition(def_id).await },
                |(builds, continuation_token)| AppMessage::BuildHistory {
                    builds,
                    continuation_token,
                },
            );
        }
        Action::FetchMoreBuilds {
            definition_id,
            continuation_token,
        } => {
            app.build_history.loading_more = true;
            spawn_api(
                client,
                tx,
                "Fetch more builds",
                move |c| async move {
                    c.list_builds_for_definition_continued(definition_id, &continuation_token)
                        .await
                },
                |(builds, continuation_token)| AppMessage::BuildHistoryMore {
                    builds,
                    continuation_token,
                },
            );
        }
        Action::FetchTimeline(build_id) => {
            spawn_timeline_fetch(client, tx, build_id, app.log_viewer.generation(), false);
        }
        Action::FetchBuildLog { build_id, log_id } => {
            spawn_log_fetch(client, tx, build_id, log_id, app.log_viewer.generation());
        }
        Action::FollowLatest => {
            // Switch to follow mode: jump cursor to active task and fetch its log.
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
        Action::DeleteRetentionLeases(ids) => {
            tracing::info!(count = ids.len(), "deleting retention leases");
            spawn_api(
                client,
                tx,
                "Delete leases",
                move |c| async move {
                    let count = ids.len() as u32;
                    c.delete_retention_leases(&ids).await.map(|()| count)
                },
                |deleted| AppMessage::RetentionLeasesDeleted { deleted, failed: 0 },
            );
        }
        Action::None => {}
    }
}
