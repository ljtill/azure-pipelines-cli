//! Dispatches user actions to state mutations and async tasks.

use std::future::Future;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::client::http::AdoClient;
use crate::events::Action;
use crate::shared::concurrency::{API_FAN_OUT_LIMIT, for_each_bounded};

use super::super::App;
use super::super::TimelineRow;
use super::super::View;
use super::super::messages::{AppMessage, RefreshSource};
use super::spawn::{
    open_url, send_app_message, spawn_api, spawn_api_with_error, spawn_build_history_fetch,
    spawn_build_history_more_fetch, spawn_build_history_refresh, spawn_data_refresh,
    spawn_fetch_boards, spawn_fetch_dashboard_pull_requests, spawn_fetch_dashboard_work_items,
    spawn_fetch_my_work_items, spawn_fetch_pinned_work_items, spawn_fetch_pr_detail,
    spawn_fetch_pull_requests, spawn_fetch_work_item_detail, spawn_log_fetch, spawn_named,
    spawn_timeline_fetch,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct BatchMutationReport {
    succeeded: u32,
    failed: u32,
}

async fn run_batch_mutation_bounded<F, Fut>(
    ids: Vec<u32>,
    limit: usize,
    operation: &'static str,
    mut task: F,
) -> BatchMutationReport
where
    F: FnMut(u32) -> Fut + Send,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let mut report = BatchMutationReport::default();

    for_each_bounded(
        ids,
        limit,
        move |id| {
            let fut = task(id);
            async move { (id, fut.await) }
        },
        |result| match result {
            Ok((id, Ok(()))) => {
                report.succeeded += 1;
                tracing::trace!(operation, item_id = id, "batch mutation succeeded");
            }
            Ok((id, Err(error))) => {
                report.failed += 1;
                tracing::warn!(operation, item_id = id, %error, "batch mutation failed");
            }
            Err(error) => {
                report.failed += 1;
                tracing::warn!(operation, %error, "batch mutation task failed");
            }
        },
    )
    .await;

    report
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
        Action::Quit | Action::Reload => {
            app.refresh.effects.cancel_all();
            app.running = false;
        }
        Action::ForceRefresh => {
            if spawn_data_refresh(app, client, tx) {
                *last_data_fetch = Instant::now();
            }
            if app.view == View::Dashboard {
                spawn_fetch_dashboard_pull_requests(app, client, tx);
                spawn_fetch_dashboard_work_items(app, client, tx);
                spawn_fetch_pinned_work_items(app, client, tx);
            }
            if app.view == View::BuildHistory {
                spawn_build_history_refresh(app, client, tx, None);
            }
            if app.view.is_pull_requests() {
                let generation = app.pull_requests.next_generation();
                spawn_fetch_pull_requests(app, client, tx, generation);
            }
            if app.view == View::Boards {
                let generation = app.boards.next_generation();
                spawn_fetch_boards(app, client, tx, generation);
            }
        }
        Action::FetchBuildHistory(def_id) => {
            let generation = app.build_history.generation;
            spawn_build_history_fetch(app, client, tx, def_id, None, generation);
        }
        Action::FetchMoreBuilds {
            definition_id,
            continuation_token,
        } => {
            app.build_history.loading_more = true;
            let generation = app.build_history.generation;
            spawn_build_history_more_fetch(
                app,
                client,
                tx,
                definition_id,
                continuation_token,
                generation,
            );
        }
        Action::FetchTimeline(build_id) => {
            let generation = app.log_viewer.generation();
            spawn_timeline_fetch(app, client, tx, build_id, generation, false);
        }
        Action::FetchBuildLog { build_id, log_id } => {
            let generation = app.log_viewer.generation();
            spawn_log_fetch(app, client, tx, build_id, log_id, generation);
        }
        Action::FollowLatest => {
            // Switch to follow mode: jump cursor to active task and fetch its log.
            if let Some((_idx, maybe_log_id)) = app.log_viewer.auto_select_log_entry() {
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
                    if let Some(build) = app.log_viewer.selected_build() {
                        let build_id = build.id;
                        let generation = app.log_viewer.generation();
                        spawn_log_fetch(app, client, tx, build_id, log_id, generation);
                    }
                } else {
                    // In-progress task with no log yet — position cursor and wait.
                    app.log_viewer.set_followed_pending(task_name);
                    app.log_viewer.clear_log();
                }
            }

            // Kick off a fresh timeline refresh so follow mode gets the latest data
            // instead of relying on the cached timeline (up to 5 seconds stale).
            if let Some(build) = app.log_viewer.selected_build() {
                let build_id = build.id;
                let generation = app.log_viewer.generation();
                spawn_timeline_fetch(app, client, tx, build_id, generation, true);
            }
        }
        Action::OpenInBrowser(url) => {
            open_url(&url);
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
            let task_tx = tx.clone();
            spawn_named("cancel_builds", tx.clone(), async move {
                let report =
                    run_batch_mutation_bounded(build_ids, API_FAN_OUT_LIMIT, "cancel_build", {
                        let client = client.clone();
                        move |id| {
                            let client = client.clone();
                            async move { client.cancel_build(id).await }
                        }
                    })
                    .await;
                send_app_message(
                    &task_tx,
                    "cancel_builds",
                    AppMessage::BuildsCancelled {
                        cancelled: report.succeeded,
                        failed: report.failed,
                    },
                )
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
            spawn_api_with_error(
                client,
                tx,
                "Approve check",
                move |c| async move {
                    c.update_approval(&approval_id, "approved", "Approved via CLI")
                        .await
                },
                |()| AppMessage::CheckUpdated,
                |e| AppMessage::RefreshError {
                    message: format!("Approve check: {e}"),
                    source: RefreshSource::Approvals,
                },
            );
        }
        Action::RejectCheck(approval_id) => {
            tracing::info!("rejecting check");
            spawn_api_with_error(
                client,
                tx,
                "Reject check",
                move |c| async move {
                    c.update_approval(&approval_id, "rejected", "Rejected via CLI")
                        .await
                },
                |()| AppMessage::CheckUpdated,
                |e| AppMessage::RefreshError {
                    message: format!("Reject check: {e}"),
                    source: RefreshSource::Approvals,
                },
            );
        }
        Action::DeleteRetentionLeases(ids) => {
            tracing::info!(count = ids.len(), "deleting retention leases");
            let client = client.clone();
            let task_tx = tx.clone();
            spawn_named("delete_retention_leases", tx.clone(), async move {
                let report =
                    run_batch_mutation_bounded(ids, API_FAN_OUT_LIMIT, "delete_retention_lease", {
                        let client = client.clone();
                        move |id| {
                            let client = client.clone();
                            async move { client.delete_retention_leases(&[id]).await }
                        }
                    })
                    .await;
                send_app_message(
                    &task_tx,
                    "delete_retention_leases",
                    AppMessage::RetentionLeasesDeleted {
                        deleted: report.succeeded,
                        failed: report.failed,
                    },
                )
                .await;
            });
        }
        Action::FetchPullRequests => {
            let generation = app.pull_requests.next_generation();
            spawn_fetch_pull_requests(app, client, tx, generation);
        }
        Action::FetchPullRequestDetail { repo_id, pr_id } => {
            spawn_fetch_pr_detail(app, client, tx, repo_id, pr_id);
        }
        Action::FetchWorkItemDetail { work_item_id } => {
            spawn_fetch_work_item_detail(app, client, tx, work_item_id);
        }
        Action::FetchDashboardPullRequests => {
            spawn_fetch_dashboard_pull_requests(app, client, tx);
            spawn_fetch_dashboard_work_items(app, client, tx);
            spawn_fetch_pinned_work_items(app, client, tx);
        }
        Action::FetchDashboardWorkItems => {
            spawn_fetch_dashboard_work_items(app, client, tx);
        }
        Action::FetchPinnedWorkItems => {
            spawn_fetch_pinned_work_items(app, client, tx);
        }
        Action::FetchBoards => {
            let generation = app.boards.next_generation();
            spawn_fetch_boards(app, client, tx, generation);
        }
        Action::FetchMyWorkItems => {
            spawn_fetch_my_work_items(app, client, tx, app.view);
        }
        Action::None => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use anyhow::anyhow;
    use tokio::sync::{Notify, mpsc};

    use super::*;

    async fn wait_for_started(rx: &mut mpsc::UnboundedReceiver<()>, expected: usize) {
        for _ in 0..expected {
            tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("batch task should start before timeout")
                .expect("started sender should remain open");
        }
    }

    async fn wait_until_released(released: Arc<AtomicBool>, release: Arc<Notify>) {
        loop {
            let notified = release.notified();
            if released.load(Ordering::SeqCst) {
                break;
            }
            notified.await;
        }
    }

    #[tokio::test]
    async fn batch_mutation_fan_out_never_exceeds_configured_concurrency() {
        let item_count = API_FAN_OUT_LIMIT + 3;
        let active = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let released = Arc::new(AtomicBool::new(false));
        let release = Arc::new(Notify::new());
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();

        let task = {
            let active = Arc::clone(&active);
            let max_seen = Arc::clone(&max_seen);
            let completed = Arc::clone(&completed);
            let released = Arc::clone(&released);
            let release = Arc::clone(&release);
            move |_id| {
                let active = Arc::clone(&active);
                let max_seen = Arc::clone(&max_seen);
                let completed = Arc::clone(&completed);
                let released = Arc::clone(&released);
                let release = Arc::clone(&release);
                let started_tx = started_tx.clone();
                async move {
                    let active_now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(active_now, Ordering::SeqCst);
                    let _ = started_tx.send(());
                    wait_until_released(released, release).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    completed.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            }
        };

        let run = tokio::spawn(run_batch_mutation_bounded(
            (1..=item_count as u32).collect(),
            API_FAN_OUT_LIMIT,
            "test_batch",
            task,
        ));

        wait_for_started(&mut started_rx, API_FAN_OUT_LIMIT).await;
        assert_eq!(completed.load(Ordering::SeqCst), 0);
        assert_eq!(max_seen.load(Ordering::SeqCst), API_FAN_OUT_LIMIT);
        assert!(started_rx.try_recv().is_err());

        released.store(true, Ordering::SeqCst);
        release.notify_waiters();

        let report = tokio::time::timeout(Duration::from_secs(2), run)
            .await
            .expect("batch mutation fan-out should finish")
            .expect("batch mutation task should not panic");

        assert_eq!(report.succeeded, item_count as u32);
        assert_eq!(report.failed, 0);
        assert!(max_seen.load(Ordering::SeqCst) <= API_FAN_OUT_LIMIT);
    }

    #[tokio::test]
    async fn batch_mutation_reports_partial_failures() {
        let report = run_batch_mutation_bounded(
            vec![1, 2, 3],
            API_FAN_OUT_LIMIT,
            "test_batch",
            |id| async move {
                if id == 2 {
                    Err(anyhow!("item {id} failed"))
                } else {
                    Ok(())
                }
            },
        )
        .await;

        assert_eq!(report.succeeded, 2);
        assert_eq!(report.failed, 1);
    }
}
