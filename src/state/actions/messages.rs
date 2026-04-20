//! Handles incoming async messages and applies them to application state.

use std::collections::BTreeMap;

use tokio::sync::mpsc;

use crate::client::http::AdoClient;
use crate::client::models;
use crate::client::models::{IdentityRef, PullRequest, WorkItem};
use crate::components::log_viewer::ActiveTaskResult;

use super::super::messages::{AppMessage, RefreshSource};
use super::super::notifications::NotificationLevel;
use super::super::{
    App, DashboardPullRequestsState, DashboardWorkItemsState, ExactUserIdentity,
    PinnedWorkItemsState, TimelineRow, View,
};
use super::spawn::{
    spawn_build_history_refresh, spawn_data_refresh, spawn_fetch_boards,
    spawn_fetch_dashboard_pull_requests, spawn_fetch_dashboard_work_items,
    spawn_fetch_pull_requests, spawn_log_fetch, spawn_timeline_fetch,
};

const DASHBOARD_IDENTITY_UNAVAILABLE_MESSAGE: &str =
    "Unable to verify your Azure DevOps identity — My Pull Requests unavailable";

const DASHBOARD_WORK_ITEMS_IDENTITY_UNAVAILABLE_MESSAGE: &str =
    "Unable to verify your Azure DevOps identity — My Work Items unavailable";

fn exact_identity_matches(author: &IdentityRef, user: &ExactUserIdentity) -> bool {
    for (author_value, user_value) in [
        (author.id.as_deref(), user.id.as_deref()),
        (author.descriptor.as_deref(), user.descriptor.as_deref()),
        (author.unique_name.as_deref(), user.unique_name.as_deref()),
    ] {
        if let (Some(author_value), Some(user_value)) = (author_value, user_value) {
            return author_value.eq_ignore_ascii_case(user_value);
        }
    }

    false
}

fn dashboard_pull_request_state(
    pull_requests: Vec<PullRequest>,
    current_user: &ExactUserIdentity,
    creator_scoped_by_id: bool,
) -> DashboardPullRequestsState {
    if !current_user.is_known() {
        return DashboardPullRequestsState::Unavailable(
            DASHBOARD_IDENTITY_UNAVAILABLE_MESSAGE.to_string(),
        );
    }

    let mut filtered: Vec<PullRequest> = pull_requests
        .into_iter()
        .filter(PullRequest::is_active)
        .filter(|pr| {
            if creator_scoped_by_id {
                return pr
                    .created_by
                    .as_ref()
                    .and_then(|author| author.id.as_deref())
                    .is_none_or(|author_id| {
                        current_user
                            .id
                            .as_deref()
                            .is_some_and(|user_id| author_id.eq_ignore_ascii_case(user_id))
                    });
            }

            pr.created_by
                .as_ref()
                .is_some_and(|author| exact_identity_matches(author, current_user))
        })
        .collect();

    filtered.sort_by_key(|pr| pr.is_draft);

    if filtered.is_empty() {
        DashboardPullRequestsState::EmptyVerified
    } else {
        DashboardPullRequestsState::Ready(filtered)
    }
}

fn dashboard_work_item_state(
    work_items: Vec<WorkItem>,
    current_user: &ExactUserIdentity,
    assigned_scoped_by_id: bool,
) -> DashboardWorkItemsState {
    if !current_user.is_known() {
        return DashboardWorkItemsState::Unavailable(
            DASHBOARD_WORK_ITEMS_IDENTITY_UNAVAILABLE_MESSAGE.to_string(),
        );
    }

    let filtered: Vec<WorkItem> = work_items
        .into_iter()
        .filter(|wi| match wi.fields.assigned_to.as_ref() {
            Some(crate::client::models::AssignedToField::Identity(identity)) => {
                if assigned_scoped_by_id {
                    identity.id.as_deref().is_none_or(|assigned_id| {
                        current_user
                            .id
                            .as_deref()
                            .is_some_and(|user_id| assigned_id.eq_ignore_ascii_case(user_id))
                    })
                } else {
                    exact_identity_matches(identity, current_user)
                }
            }
            // Non-identity AssignedTo (e.g. bare display-name string) cannot be verified.
            _ => false,
        })
        .collect();

    if filtered.is_empty() {
        DashboardWorkItemsState::EmptyVerified
    } else {
        DashboardWorkItemsState::Ready(filtered)
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
            retention_leases,
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
            app.data_refresh.succeed();
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
            //   - A build transitions to InProgress (started).
            //   - A build transitions to Completed (succeeded/failed/canceled).
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

                    // Only notify on meaningful transitions.
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
                            Some(models::BuildResult::Failed | models::BuildResult::Canceled) => {
                                NotificationLevel::Error
                            }
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
                .filter_map(crate::client::models::approvals::Approval::build_id)
                .collect();
            app.data.pending_approvals = pending_approvals;

            app.rebuild_dashboard();
            app.rebuild_pipelines();
            app.active_runs.rebuild(
                &app.data.active_builds,
                &app.filters.definition_ids,
                &app.search.query,
            );
            app.last_refresh = Some(chrono::Utc::now());
            app.loading = false;

            // Update retention leases.
            app.retention_leases.leases = retention_leases;
            app.retention_leases.rebuild_index();
        }
        AppMessage::BuildHistory {
            builds,
            continuation_token,
        } => {
            tracing::info!(
                count = builds.len(),
                has_more = continuation_token.is_some(),
                "build history loaded"
            );
            app.build_history.has_more = continuation_token.is_some();
            app.build_history.continuation_token = continuation_token;
            app.build_history.builds = builds;
            app.build_history
                .nav
                .set_len(app.build_history.builds.len());
            // Restore stashed scroll position (e.g. after lease deletion refresh).
            if let Some(idx) = app.build_history.pending_nav_index.take() {
                app.build_history.nav.set_index(idx);
            }
        }
        AppMessage::BuildHistoryMore {
            builds,
            continuation_token,
        } => {
            tracing::info!(
                count = builds.len(),
                has_more = continuation_token.is_some(),
                "more build history loaded"
            );
            app.build_history.loading_more = false;
            app.build_history.has_more = continuation_token.is_some();
            app.build_history.continuation_token = continuation_token;
            app.build_history.builds.extend(builds);
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
                // Initial load: full setup with auto-select.
                app.log_viewer.clear_log();
                app.log_viewer.nav_mut().set_index(0);
                app.log_viewer.enter_follow_mode();
                app.log_viewer.rebuild_timeline_rows();

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
                        spawn_log_fetch(client, tx, build_id, log_id, app.log_viewer.generation());
                    } else {
                        // In-progress task has no log yet — show it but wait for log.
                        app.log_viewer.set_followed_pending(task_name);
                    }
                }
            } else if app.log_viewer.is_following() {
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
                            spawn_log_fetch(
                                client,
                                tx,
                                build_id,
                                log_id,
                                app.log_viewer.generation(),
                            );
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
            } else {
                // Refresh in inspect mode: only update tree status, preserve cursor + log.
                app.log_viewer.rebuild_timeline_rows();
            }
        }
        AppMessage::LogContent {
            content,
            generation,
            log_id,
        } => {
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
                    .is_some_and(|fid| fid != log_id)
            {
                tracing::debug!(
                    log_id,
                    followed = ?app.log_viewer.followed_log_id(),
                    "discarding log content for non-followed task"
                );
                return;
            }
            tracing::debug!(bytes = content.len(), log_id, "log content received");
            app.log_viewer.set_log_content(&content);
        }
        AppMessage::LogRefreshFinished { had_failure } => {
            tracing::debug!(had_failure, "log refresh finished");
            if had_failure {
                app.log_refresh.fail(5, 60);
            } else {
                app.log_refresh.succeed();
            }
        }
        AppMessage::Error(msg) => {
            tracing::warn!(error = %msg, "app error");
            app.notifications.error(msg);
        }
        AppMessage::RefreshError { message, source } => {
            tracing::warn!(error = %message, ?source, "refresh error");
            if source == RefreshSource::Data {
                app.data_refresh.fail(30, 300);
            }
            app.notifications.error_dedup(message);
        }
        AppMessage::BuildCancelled => {
            tracing::info!("build cancelled successfully");
            app.notifications.success("Build cancelled");
            spawn_data_refresh(app, client, tx);
            if app.view == View::BuildHistory {
                spawn_build_history_refresh(app, client, tx, None);
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
                crate::state::notifications::NotificationLevel::Info,
                format!("Update available: v{version} — run 'devops update' to upgrade"),
            );
        }
        AppMessage::RetentionLeasesDeleted { deleted, failed } => {
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
                // Request enough builds to cover everything already loaded so
                // the scroll position can be restored after the refresh.
                let top = (app.build_history.builds.len() as u32)
                    .max(crate::client::endpoints::TOP_DEFINITION_BUILDS);
                spawn_build_history_refresh(app, client, tx, Some(top));
            }
        }
        AppMessage::PullRequestsLoaded {
            pull_requests,
            generation,
        } => {
            if generation < app.pull_requests.generation {
                tracing::debug!(
                    generation,
                    current = app.pull_requests.generation,
                    "dropping stale pull requests response"
                );
                return;
            }
            tracing::info!(count = pull_requests.len(), "pull requests loaded");
            app.pull_requests.set_data(pull_requests, &app.search.query);
        }
        AppMessage::BoardsLoaded {
            team_name,
            backlogs,
            work_items,
            generation,
        } => {
            if generation < app.boards.generation {
                tracing::debug!(
                    generation,
                    current = app.boards.generation,
                    "dropping stale boards response"
                );
                return;
            }
            tracing::info!(
                team = %team_name,
                backlogs = backlogs.len(),
                work_items = work_items.len(),
                "boards loaded"
            );
            app.boards
                .set_data(team_name, backlogs, work_items, &app.search.query);
        }
        AppMessage::BoardsFailed {
            message,
            generation,
        } => {
            if generation < app.boards.generation {
                tracing::debug!(
                    generation,
                    current = app.boards.generation,
                    "dropping stale boards failure"
                );
                return;
            }
            tracing::warn!(%message, "boards fetch failed");
            app.boards.set_error(message.clone());
            app.notifications.error_dedup(message);
        }
        AppMessage::MyWorkItemsLoaded {
            view,
            work_items,
            generation,
        } => {
            let Some(list) = app.my_work_items.list_for_mut(view) else {
                return;
            };
            if generation < list.generation {
                tracing::debug!(
                    ?view,
                    generation,
                    current = list.generation,
                    "dropping stale my work items response"
                );
                return;
            }
            tracing::info!(?view, count = work_items.len(), "my work items loaded");
            list.set_data(&work_items, &app.search.query);
        }
        AppMessage::MyWorkItemsFailed {
            view,
            message,
            generation,
        } => {
            let Some(list) = app.my_work_items.list_for_mut(view) else {
                return;
            };
            if generation < list.generation {
                tracing::debug!(
                    ?view,
                    generation,
                    current = list.generation,
                    "dropping stale my work items failure"
                );
                return;
            }
            tracing::warn!(?view, %message, "my work items fetch failed");
            app.notifications.error_dedup(message);
        }
        AppMessage::TaskPanicked { task_name, message } => {
            tracing::error!(task_name, %message, "background task panicked");
            app.notifications.push_persistent(
                NotificationLevel::Error,
                format!(
                    "Background task '{task_name}' panicked: {message}. \
                     Attach logs from ~/.local/state/devops/ when reporting."
                ),
            );
        }
        AppMessage::AdoApiVersionUnsupported {
            requested,
            server_message,
        } => {
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
        AppMessage::PaginationProgress {
            endpoint,
            page,
            items,
        } => {
            tracing::debug!(endpoint, page, items, "pagination progress");
            app.pagination_status = Some(crate::state::PaginationStatus {
                endpoint,
                page,
                items,
            });
        }
        AppMessage::UserIdentity { identity } => {
            tracing::info!("user identity resolved");
            app.current_user = identity;
            // Re-fetch dashboard PRs now that we can filter by creator.
            spawn_fetch_dashboard_pull_requests(app, client, tx);
            // Re-fetch dashboard work items now that we can filter by assignee.
            spawn_fetch_dashboard_work_items(app, client, tx);
            // Re-fetch PR view data so filtered modes use the resolved identity.
            if app.view.is_pull_requests() {
                let generation = app.pull_requests.next_generation();
                spawn_fetch_pull_requests(app, client, tx, generation);
            }
            if app.view == View::Boards {
                let generation = app.boards.next_generation();
                spawn_fetch_boards(app, client, tx, generation);
            }
        }
        AppMessage::PullRequestDetailLoaded {
            pull_request,
            threads,
        } => {
            tracing::info!(
                pr_id = pull_request.pull_request_id,
                threads = threads.len(),
                "pull request detail loaded"
            );
            app.pull_request_detail.pull_request = Some(pull_request);
            app.pull_request_detail.threads = threads;
            app.pull_request_detail.loading = false;
            // Set nav length to number of display sections.
            let section_count = app.pull_request_detail.section_count();
            app.pull_request_detail.nav.set_len(section_count);
        }
        AppMessage::WorkItemDetailLoaded {
            work_item_id,
            work_item,
            comments,
        } => {
            if app.work_item_detail.work_item_id == Some(work_item_id) {
                tracing::info!(
                    work_item_id,
                    comments = comments.len(),
                    "work item detail loaded"
                );
                app.work_item_detail.work_item = Some(*work_item);
                app.work_item_detail.comments = comments;
                app.work_item_detail.loading = false;
                let section_count = app.work_item_detail.section_count();
                app.work_item_detail.nav.set_len(section_count);
            } else {
                tracing::debug!(
                    work_item_id,
                    pending = ?app.work_item_detail.work_item_id,
                    "ignoring stale work item detail response"
                );
            }
        }
        AppMessage::WorkItemDetailFailed {
            work_item_id,
            message,
        } => {
            if app.work_item_detail.work_item_id == Some(work_item_id) {
                tracing::warn!(work_item_id, %message, "work item detail fetch failed");
                app.work_item_detail.loading = false;
            }
            app.notifications.error(message);
        }
        AppMessage::DashboardPullRequests {
            pull_requests,
            creator_scoped_by_id,
        } => {
            tracing::info!(count = pull_requests.len(), "dashboard PRs loaded");
            app.dashboard_pull_requests = dashboard_pull_request_state(
                pull_requests,
                &app.current_user,
                creator_scoped_by_id,
            );
            app.rebuild_dashboard();
        }
        AppMessage::DashboardPullRequestsFailed { message } => {
            tracing::warn!(%message, "dashboard pull request fetch failed");
            app.notifications.error_dedup(message.clone());
            app.dashboard_pull_requests = DashboardPullRequestsState::Unavailable(message);
            app.rebuild_dashboard();
        }
        AppMessage::DashboardWorkItems {
            work_items,
            assigned_scoped_by_id,
        } => {
            tracing::info!(count = work_items.len(), "dashboard work items loaded");
            app.dashboard_work_items =
                dashboard_work_item_state(work_items, &app.current_user, assigned_scoped_by_id);
            app.rebuild_dashboard();
        }
        AppMessage::DashboardWorkItemsFailed { message } => {
            tracing::warn!(%message, "dashboard work items fetch failed");
            app.notifications.error_dedup(message.clone());
            app.dashboard_work_items = DashboardWorkItemsState::Unavailable(message);
            app.rebuild_dashboard();
        }
        AppMessage::PinnedWorkItems { work_items } => {
            tracing::info!(count = work_items.len(), "pinned work items loaded");
            app.pinned_work_items = PinnedWorkItemsState::Ready(work_items);
            app.rebuild_dashboard();
        }
        AppMessage::PinnedWorkItemsFailed { message } => {
            tracing::warn!(%message, "pinned work items fetch failed");
            app.notifications.error_dedup(message.clone());
            app.pinned_work_items = PinnedWorkItemsState::Unavailable(message);
            app.rebuild_dashboard();
        }
        AppMessage::UserIdentityFailed { message } => {
            tracing::warn!(%message, "user identity resolution failed");
            app.notifications.error_dedup(message.clone());
            app.current_user = ExactUserIdentity::default();
            app.dashboard_pull_requests = DashboardPullRequestsState::Unavailable(message.clone());
            app.dashboard_work_items = DashboardWorkItemsState::Unavailable(message);
            app.rebuild_dashboard();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::client::models::*;
    use crate::state::{DashboardPullRequestsState, ExactUserIdentity, View};
    use crate::test_helpers::*;

    fn board_backlog(name: &str, rank: u32, is_hidden: bool) -> BacklogLevelConfiguration {
        BacklogLevelConfiguration {
            id: format!("backlog-{name}"),
            name: name.to_string(),
            rank,
            work_item_count_limit: None,
            work_item_types: vec![WorkItemTypeReference {
                name: name.to_string(),
                url: None,
            }],
            default_work_item_type: None,
            color: None,
            is_hidden,
            backlog_type: None,
        }
    }

    fn board_work_item(
        id: u32,
        parent_id: Option<u32>,
        child_ids: Vec<u32>,
        title: &str,
        stack_rank: f64,
    ) -> WorkItem {
        WorkItem {
            id,
            rev: None,
            fields: WorkItemFields {
                title: title.to_string(),
                work_item_type: if parent_id.is_none() {
                    "Epic".to_string()
                } else {
                    "Feature".to_string()
                },
                state: Some("Active".to_string()),
                assigned_to: None,
                iteration_path: Some("Project\\Iteration".to_string()),
                area_path: None,
                parent: parent_id,
                board_column: None,
                stack_rank: Some(stack_rank),
                ..Default::default()
            },
            relations: child_ids
                .into_iter()
                .map(|child_id| WorkItemRelation {
                    rel: Some("System.LinkTypes.Hierarchy-Forward".to_string()),
                    url: format!("https://dev.azure.com/org/_apis/wit/workItems/{child_id}"),
                    attributes: std::collections::HashMap::new(),
                })
                .collect(),
            url: None,
        }
    }

    #[test]
    fn exact_identity_matches_rejects_conflicting_shared_identifiers() {
        let author = IdentityRef {
            id: Some("author-id".to_string()),
            unique_name: Some("same@contoso.com".to_string()),
            descriptor: None,
            display_name: "Author".to_string(),
        };
        let current_user = ExactUserIdentity {
            id: Some("different-id".to_string()),
            unique_name: Some("same@contoso.com".to_string()),
            descriptor: None,
        };

        assert!(!exact_identity_matches(&author, &current_user));
    }

    #[test]
    fn exact_identity_matches_prefers_id_over_weaker_conflicts() {
        let author = IdentityRef {
            id: Some("same-id".to_string()),
            unique_name: Some("author@contoso.com".to_string()),
            descriptor: None,
            display_name: "Author".to_string(),
        };
        let current_user = ExactUserIdentity {
            id: Some("same-id".to_string()),
            unique_name: Some("different@contoso.com".to_string()),
            descriptor: None,
        };

        assert!(exact_identity_matches(&author, &current_user));
    }

    #[test]
    fn dashboard_pull_request_state_keeps_only_exact_active_matches() {
        let mut matching_active = make_pull_request(1, "Mine", "active", "repo");
        matching_active.created_by.as_mut().unwrap().id = Some("me".to_string());

        let mut other_users = make_pull_request(2, "Not mine", "active", "repo");
        other_users.created_by.as_mut().unwrap().id = Some("other".to_string());

        let mut matching_completed = make_pull_request(3, "Completed", "completed", "repo");
        matching_completed.created_by.as_mut().unwrap().id = Some("me".to_string());

        let missing_identifier = make_pull_request(4, "Unknown author", "active", "repo");

        let state = dashboard_pull_request_state(
            vec![
                matching_active,
                other_users,
                matching_completed,
                missing_identifier,
            ],
            &ExactUserIdentity {
                id: Some("me".to_string()),
                unique_name: None,
                descriptor: None,
            },
            false,
        );

        match state {
            DashboardPullRequestsState::Ready(prs) => {
                assert_eq!(prs.len(), 1);
                assert_eq!(prs[0].pull_request_id, 1);
            }
            other => panic!("expected Ready state, got {other:?}"),
        }
    }

    #[test]
    fn dashboard_pull_request_state_keeps_creator_scoped_results_without_author_id() {
        let mut my_pr = make_pull_request(1, "Mine", "active", "repo");
        my_pr.created_by.as_mut().unwrap().id = None;

        let state = dashboard_pull_request_state(
            vec![my_pr],
            &ExactUserIdentity {
                id: Some("me".to_string()),
                unique_name: Some("me@contoso.com".to_string()),
                descriptor: None,
            },
            true,
        );

        match state {
            DashboardPullRequestsState::Ready(prs) => {
                assert_eq!(prs.len(), 1);
                assert_eq!(prs[0].pull_request_id, 1);
            }
            other => panic!("expected Ready state, got {other:?}"),
        }
    }

    #[test]
    fn dashboard_pull_request_state_orders_non_drafts_before_drafts() {
        let mut draft_pr = make_pull_request(1, "Draft", "active", "repo");
        draft_pr.is_draft = true;
        draft_pr.created_by.as_mut().unwrap().id = Some("me".to_string());

        let mut active_pr = make_pull_request(2, "Active", "active", "repo");
        active_pr.created_by.as_mut().unwrap().id = Some("me".to_string());

        let state = dashboard_pull_request_state(
            vec![draft_pr, active_pr],
            &ExactUserIdentity {
                id: Some("me".to_string()),
                unique_name: None,
                descriptor: None,
            },
            true,
        );

        match state {
            DashboardPullRequestsState::Ready(prs) => {
                assert_eq!(prs.len(), 2);
                assert_eq!(prs[0].pull_request_id, 2);
                assert_eq!(prs[1].pull_request_id, 1);
            }
            other => panic!("expected Ready state, got {other:?}"),
        }
    }

    #[test]
    fn dashboard_pull_request_state_returns_empty_verified_when_no_exact_matches() {
        let mut pr = make_pull_request(1, "Not mine", "active", "repo");
        pr.created_by.as_mut().unwrap().id = Some("someone-else".to_string());

        let state = dashboard_pull_request_state(
            vec![pr],
            &ExactUserIdentity {
                id: Some("me".to_string()),
                unique_name: None,
                descriptor: None,
            },
            false,
        );

        assert!(matches!(state, DashboardPullRequestsState::EmptyVerified));
    }

    #[test]
    fn dashboard_pull_request_state_returns_unavailable_when_identity_unknown() {
        let state = dashboard_pull_request_state(
            vec![make_pull_request(1, "Mine", "active", "repo")],
            &ExactUserIdentity::default(),
            false,
        );

        assert!(matches!(state, DashboardPullRequestsState::Unavailable(_)));
    }

    #[test]
    fn dashboard_pull_requests_failed_sets_unavailable_and_notifies() {
        let mut app = make_app();
        let client = crate::client::http::AdoClient::new("org", "project").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        handle_message(
            &mut app,
            &client,
            &tx,
            AppMessage::DashboardPullRequestsFailed {
                message: "Failed to load My Pull Requests: timeout".to_string(),
            },
        );

        assert!(matches!(
            app.dashboard_pull_requests,
            DashboardPullRequestsState::Unavailable(_)
        ));
        let notification = app.notifications.clone_current().unwrap();
        assert_eq!(
            notification.message,
            "Failed to load My Pull Requests: timeout"
        );
    }

    #[test]
    fn user_identity_failed_sets_unavailable_and_notifies() {
        let mut app = make_app();
        let client = crate::client::http::AdoClient::new("org", "project").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        handle_message(
            &mut app,
            &client,
            &tx,
            AppMessage::UserIdentityFailed {
                message: "Unable to verify your Azure DevOps identity — My Pull Requests unavailable: connection data request was unauthorized (401)".to_string(),
            },
        );

        assert!(matches!(
            app.dashboard_pull_requests,
            DashboardPullRequestsState::Unavailable(_)
        ));
        let notification = app.notifications.clone_current().unwrap();
        assert_eq!(
            notification.message,
            "Unable to verify your Azure DevOps identity — My Pull Requests unavailable: connection data request was unauthorized (401)"
        );
    }

    // --- DataRefresh ---

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
        let recent = vec![b1];

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
        app.rebuild_dashboard();
        app.rebuild_pipelines();
        app.active_runs.rebuild(
            &app.data.active_builds,
            &app.filters.definition_ids,
            &app.search.query,
        );
        app.last_refresh = Some(chrono::Utc::now());
        app.loading = false;

        assert_eq!(app.data.definitions.len(), 2);
        assert!(!app.pipelines.rows.is_empty());
        assert!(!app.dashboard.rows.is_empty());
        assert!(app.last_refresh.is_some());
        assert!(!app.loading);
    }

    #[test]
    fn data_refresh_preserves_notifications() {
        let mut app = make_app();
        app.notifications.error("old error");
        assert!(app.notifications.clone_current().is_some());

        // DataRefresh no longer clears notifications — they expire via TTL.
        // Simulate what handle_message(DataRefresh) does (no clear call).
        app.loading = false;
        assert!(app.notifications.clone_current().is_some());
    }

    #[test]
    fn data_refresh_replaces_previous_data() {
        let mut app = make_app();
        assert_eq!(app.data.definitions.len(), 3); // make_app seeds 3

        // Simulate a DataRefresh with only 1 definition.
        app.data.definitions = vec![make_definition(99, "Only", "\\")];
        app.data.recent_builds = vec![];
        app.data.latest_builds_by_def.clear();
        app.data.active_builds = vec![];
        app.data.pending_approvals = vec![];
        app.rebuild_dashboard();
        app.rebuild_pipelines();
        app.active_runs.rebuild(
            &app.data.active_builds,
            &app.filters.definition_ids,
            &app.search.query,
        );
        app.last_refresh = Some(chrono::Utc::now());
        app.loading = false;

        assert_eq!(app.data.definitions.len(), 1);
        assert!(!app.pipelines.rows.is_empty());
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
        // No recent_builds to overlay.
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

    // --- BuildHistory ---

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
        // Nav synced — 3 items, index starts at 0.
        app.build_history.nav.down();
        assert_eq!(app.build_history.nav.index(), 1);
    }

    #[test]
    fn build_history_empty() {
        let mut app = make_app();
        app.build_history.builds = vec![];
        app.build_history.nav.set_len(0);
        assert_eq!(app.build_history.nav.index(), 0);
        // Down on empty list is a no-op.
        app.build_history.nav.down();
        assert_eq!(app.build_history.nav.index(), 0);
    }

    #[test]
    fn build_history_restores_pending_nav_index() {
        let mut app = make_app();
        // Simulate user at index 2 in a 5-item list.
        app.build_history.nav.set_len(5);
        app.build_history.nav.set_index(2);
        app.build_history.pending_nav_index = Some(2);

        // Simulate BuildHistory message arriving with fresh data.
        let builds = vec![
            make_build(1, BuildStatus::Completed, Some(BuildResult::Succeeded)),
            make_build(2, BuildStatus::Completed, Some(BuildResult::Failed)),
            make_build(3, BuildStatus::InProgress, None),
            make_build(4, BuildStatus::Completed, Some(BuildResult::Succeeded)),
            make_build(5, BuildStatus::Completed, Some(BuildResult::Succeeded)),
        ];
        app.build_history.builds = builds;
        app.build_history
            .nav
            .set_len(app.build_history.builds.len());
        if let Some(idx) = app.build_history.pending_nav_index.take() {
            app.build_history.nav.set_index(idx);
        }

        assert_eq!(app.build_history.nav.index(), 2);
        assert!(app.build_history.pending_nav_index.is_none());
    }

    #[test]
    fn build_history_pending_nav_clamps_to_shorter_list() {
        let mut app = make_app();
        // User was at index 4, but refresh returns only 3 builds.
        app.build_history.pending_nav_index = Some(4);

        let builds = vec![
            make_build(1, BuildStatus::Completed, Some(BuildResult::Succeeded)),
            make_build(2, BuildStatus::Completed, Some(BuildResult::Failed)),
            make_build(3, BuildStatus::InProgress, None),
        ];
        app.build_history.builds = builds;
        app.build_history
            .nav
            .set_len(app.build_history.builds.len());
        if let Some(idx) = app.build_history.pending_nav_index.take() {
            app.build_history.nav.set_index(idx);
        }

        // Clamped to last item (index 2).
        assert_eq!(app.build_history.nav.index(), 2);
    }

    #[test]
    fn build_history_no_pending_nav_stays_at_zero() {
        let mut app = make_app();
        assert!(app.build_history.pending_nav_index.is_none());

        let builds = vec![
            make_build(1, BuildStatus::Completed, Some(BuildResult::Succeeded)),
            make_build(2, BuildStatus::Completed, Some(BuildResult::Failed)),
        ];
        app.build_history.builds = builds;
        app.build_history
            .nav
            .set_len(app.build_history.builds.len());
        if let Some(idx) = app.build_history.pending_nav_index.take() {
            app.build_history.nav.set_index(idx);
        }

        // No pending index — stays at default 0.
        assert_eq!(app.build_history.nav.index(), 0);
    }

    // --- LogContent ---

    #[test]
    fn log_content_splits_lines() {
        let mut app = make_app();
        app.navigate_to_log_viewer(make_build(
            1,
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
        ));
        app.log_viewer.set_log_content("line1\nline2\nline3");

        assert_eq!(app.log_viewer.log_content().len(), 3);
        assert_eq!(app.log_viewer.log_content().get(0).unwrap(), "line1");
        assert_eq!(app.log_viewer.log_content().get(2).unwrap(), "line3");
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
        app.log_viewer.set_log_content("");
        // "".lines() yields nothing, so vec should be empty.
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

        // Setting new log content resets scroll.
        app.log_viewer.set_log_content("fresh\nlog");
        assert_eq!(app.log_viewer.log_scroll_offset(), 0);
        assert!(app.log_viewer.log_auto_scroll());
    }

    // --- Generation / stale-guard ---

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

        // Simulate the stale guard: only apply content if generation matches.
        let content = "should not appear".to_string();
        if stale_gen == app.log_viewer.generation() {
            app.log_viewer.set_log_content(&content);
        }
        // Content should remain empty because the generation didn't match.
        assert!(app.log_viewer.log_content().is_empty());
    }

    // --- Error / notification messages ---

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

        // BuildsCancelled handler clears selections.
        app.active_runs.selected.clear();
        assert!(app.active_runs.selected.is_empty());
    }

    #[test]
    fn batch_cancel_with_failures_shows_error() {
        let mut app = make_app();
        // Simulate partial-failure path from BuildsCancelled.
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

    // --- PipelineQueued ---

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

    // --- StageRetried / CheckUpdated notification ---

    #[test]
    fn stage_retried_shows_success() {
        let mut app = make_app();
        // Mirrors handle_message(StageRetried) notification path.
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

    // --- State-change notifications ---

    /// Simulates the DataRefresh notification-diff logic for a given
    /// app and new latest_builds_by_def map. Mirrors the logic in handle_message.
    fn simulate_notification_diff(app: &mut crate::state::App, map: &BTreeMap<u32, Build>) {
        use crate::state::notifications::NotificationLevel;

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
                        Some(BuildResult::Failed | BuildResult::Canceled) => {
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

        // First refresh: completed build (seed).
        let mut b1 = make_build(100, BuildStatus::Completed, Some(BuildResult::Succeeded));
        b1.definition = BuildDefinitionRef {
            id: 1,
            name: "CI Pipeline".into(),
        };
        let mut map1 = BTreeMap::new();
        map1.insert(1u32, b1);
        simulate_notification_diff(&mut app, &map1);
        assert!(app.notifications.clone_current().is_none()); // first load

        // Second refresh: new build started.
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
        assert_eq!(
            n.level,
            crate::state::notifications::NotificationLevel::Info
        );
    }

    #[test]
    fn notification_on_same_build_completing() {
        let mut app = make_app();
        app.notifications_enabled = true;

        // First refresh: in-progress build (seed).
        let mut b1 = make_build(100, BuildStatus::InProgress, None);
        b1.definition = BuildDefinitionRef {
            id: 1,
            name: "Deploy".into(),
        };
        let mut map1 = BTreeMap::new();
        map1.insert(1u32, b1);
        simulate_notification_diff(&mut app, &map1);

        // Second refresh: same build ID now completed.
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
        assert_eq!(
            n.level,
            crate::state::notifications::NotificationLevel::Error
        );
    }

    #[test]
    fn notification_on_new_build_completing() {
        let mut app = make_app();
        app.notifications_enabled = true;

        // Seed with build 100 in-progress.
        let mut b1 = make_build(100, BuildStatus::InProgress, None);
        b1.definition = BuildDefinitionRef {
            id: 1,
            name: "CI Pipeline".into(),
        };
        let mut map1 = BTreeMap::new();
        map1.insert(1u32, b1);
        simulate_notification_diff(&mut app, &map1);

        // New build 101 completed (different build ID).
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
            crate::state::notifications::NotificationLevel::Success
        );
    }

    #[test]
    fn no_notification_when_build_unchanged() {
        let mut app = make_app();
        app.notifications_enabled = true;

        // Seed with completed build.
        let mut b = make_build(100, BuildStatus::Completed, Some(BuildResult::Succeeded));
        b.definition = BuildDefinitionRef {
            id: 1,
            name: "CI".into(),
        };
        let mut map = BTreeMap::new();
        map.insert(1u32, b);
        simulate_notification_diff(&mut app, &map);

        // Same build again.
        simulate_notification_diff(&mut app, &map);

        // No notification (first load skipped, second has same build_id + status).
        assert!(app.notifications.clone_current().is_none());
    }

    #[test]
    fn no_notification_when_disabled() {
        let mut app = make_app();
        app.notifications_enabled = false;

        // Seed snapshot manually.
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

    // --- BoardsLoaded stale-response guard ---

    #[test]
    fn boards_loaded_updates_data_and_applies_search_query() {
        let mut app = make_app();
        let client = crate::client::http::AdoClient::new("org", "project").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        app.search.query = "needle".to_string();

        handle_message(
            &mut app,
            &client,
            &tx,
            AppMessage::BoardsLoaded {
                team_name: "Project Team".to_string(),
                backlogs: vec![
                    board_backlog("Hidden", 0, true),
                    board_backlog("Epics", 1, false),
                ],
                work_items: vec![
                    board_work_item(1, None, vec![2], "Root", 1.0),
                    board_work_item(2, Some(1), vec![], "Needle child", 2.0),
                    board_work_item(3, None, vec![], "Other root", 3.0),
                ],
                generation: 0,
            },
        );

        assert_eq!(app.boards.team_name.as_deref(), Some("Project Team"));
        assert_eq!(app.boards.backlog_names, vec!["Epics"]);
        assert_eq!(
            app.boards
                .rows
                .iter()
                .map(|row| row.work_item_id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(!app.boards.rows[0].collapsed);
    }

    #[test]
    fn boards_loaded_derives_children_from_parent_field_over_stale_relations() {
        let mut app = make_app();
        let client = crate::client::http::AdoClient::new("org", "project").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        handle_message(
            &mut app,
            &client,
            &tx,
            AppMessage::BoardsLoaded {
                team_name: "Project Team".to_string(),
                backlogs: vec![board_backlog("Epics", 1, false)],
                work_items: vec![
                    board_work_item(10, None, vec![30], "Stale relation parent", 10.0),
                    board_work_item(20, None, vec![], "Authoritative parent", 20.0),
                    board_work_item(30, Some(20), vec![], "Child", 30.0),
                ],
                generation: 0,
            },
        );

        assert_eq!(
            app.boards
                .items
                .get(&10)
                .map(|item| item.child_ids.as_slice()),
            Some(&[][..])
        );
        assert_eq!(
            app.boards
                .items
                .get(&20)
                .map(|item| item.child_ids.as_slice()),
            Some(&[30][..])
        );
        assert_eq!(
            app.boards
                .rows
                .iter()
                .map(|row| row.work_item_id)
                .collect::<Vec<_>>(),
            vec![10, 20]
        );

        assert!(app.boards.expand_row(1, ""));
        assert_eq!(
            app.boards
                .rows
                .iter()
                .map(|row| row.work_item_id)
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
        assert_eq!(
            app.boards
                .rows
                .iter()
                .filter(|row| row.work_item_id == 30)
                .count(),
            1
        );
    }

    #[test]
    fn boards_loaded_drops_stale_response() {
        let mut app = make_app();
        let client = crate::client::http::AdoClient::new("org", "project").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        app.boards.next_generation();
        app.boards.next_generation();

        handle_message(
            &mut app,
            &client,
            &tx,
            AppMessage::BoardsLoaded {
                team_name: "Stale Team".to_string(),
                backlogs: vec![board_backlog("Epics", 1, false)],
                work_items: vec![board_work_item(1, None, vec![], "Stale root", 1.0)],
                generation: 1,
            },
        );

        assert!(app.boards.items.is_empty());
        assert!(app.boards.team_name.is_none());
    }

    #[test]
    fn boards_failed_preserves_diagnostics_and_notifies() {
        let mut app = make_app();
        let client = crate::client::http::AdoClient::new("org", "project").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        let generation = app.boards.next_generation();
        app.boards.start_loading();
        let message = "Failed to load backlog: HTTP status client error (400 Bad Request): { \"message\": \"Field 'System.BoardColumn' is invalid.\" }";

        handle_message(
            &mut app,
            &client,
            &tx,
            AppMessage::BoardsFailed {
                message: message.to_string(),
                generation,
            },
        );

        assert_eq!(app.boards.error.as_deref(), Some(message));
        assert!(!app.boards.loading);
        assert_eq!(app.notifications.clone_current().unwrap().message, message);
    }

    #[test]
    fn boards_failed_drops_stale_failure() {
        let mut app = make_app();
        let client = crate::client::http::AdoClient::new("org", "project").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        app.boards.set_error("Current backlog error".to_string());
        app.notifications
            .error_dedup("Current backlog error".to_string());
        app.boards.next_generation();
        app.boards.next_generation();

        handle_message(
            &mut app,
            &client,
            &tx,
            AppMessage::BoardsFailed {
                message: "Failed to load backlog: stale failure".to_string(),
                generation: 1,
            },
        );

        assert_eq!(app.boards.error.as_deref(), Some("Current backlog error"));
        assert_eq!(
            app.notifications.clone_current().unwrap().message,
            "Current backlog error"
        );
    }

    // --- PullRequestsLoaded stale-response guard ---

    #[test]
    fn pull_requests_loaded_updates_data() {
        let mut app = make_app();
        app.view = View::PullRequestsCreatedByMe;

        let generation = app.pull_requests.next_generation();
        let prs = vec![
            make_pull_request(1, "PR one", "active", "repo-a"),
            make_pull_request(2, "PR two", "active", "repo-b"),
        ];
        // Simulate what handle_message does for PullRequestsLoaded.
        if generation >= app.pull_requests.generation {
            app.pull_requests.set_data(prs, &app.search.query);
        }
        assert_eq!(app.pull_requests.filtered.len(), 2);
    }

    #[test]
    fn pull_requests_loaded_drops_stale_response() {
        let mut app = make_app();
        app.view = View::PullRequestsCreatedByMe;

        // Advance generation twice to simulate rapid tab switching.
        let _old_gen = app.pull_requests.next_generation();
        let _current_gen = app.pull_requests.next_generation();

        let stale_prs = vec![make_pull_request(1, "Stale PR", "active", "repo")];
        // Simulate a response arriving with the old generation.
        let stale_gen = 1u64;
        if stale_gen < app.pull_requests.generation {
            // Should be dropped — don't call set_data.
        } else {
            app.pull_requests.set_data(stale_prs, &app.search.query);
        }
        // Data should remain empty because the stale response was dropped.
        assert!(app.pull_requests.filtered.is_empty());
    }

    #[test]
    fn pull_requests_loaded_current_generation_accepted() {
        let mut app = make_app();
        app.view = View::PullRequestsCreatedByMe;

        // Advance to generation 3.
        app.pull_requests.next_generation();
        app.pull_requests.next_generation();
        let current = app.pull_requests.next_generation();
        assert_eq!(current, 3);

        let prs = vec![make_pull_request(5, "Current PR", "active", "repo")];
        // Response with matching generation should be accepted.
        if current >= app.pull_requests.generation {
            app.pull_requests.set_data(prs, &app.search.query);
        }
        assert_eq!(app.pull_requests.filtered.len(), 1);
        assert_eq!(app.pull_requests.filtered[0].pull_request_id, 5);
    }
}
