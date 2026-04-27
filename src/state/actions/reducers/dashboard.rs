//! Dashboard reducer logic for identity-scoped dashboard sections.

use tokio::sync::mpsc;

use crate::client::http::AdoClient;
use crate::client::models::{AssignedToField, IdentityRef, PullRequest, WorkItem};
use crate::state::actions::spawn::{
    spawn_fetch_boards, spawn_fetch_dashboard_pull_requests, spawn_fetch_dashboard_work_items,
    spawn_fetch_pinned_work_items, spawn_fetch_pull_requests,
};
use crate::state::messages::AppMessage;
use crate::state::{
    App, DashboardPullRequestsState, DashboardWorkItemsState, ExactUserIdentity,
    PinnedWorkItemsState, View,
};

const DASHBOARD_IDENTITY_UNAVAILABLE_MESSAGE: &str =
    "Unable to verify your Azure DevOps identity — My Pull Requests unavailable";

const DASHBOARD_WORK_ITEMS_IDENTITY_UNAVAILABLE_MESSAGE: &str =
    "Unable to verify your Azure DevOps identity — My Work Items unavailable";

pub(in crate::state::actions) fn exact_identity_matches(
    author: &IdentityRef,
    user: &ExactUserIdentity,
) -> bool {
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

pub(in crate::state::actions) fn dashboard_pull_request_state(
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

    normalize_pull_requests(&mut filtered);
    filtered.sort_by_key(|pr| pr.is_draft);

    if filtered.is_empty() {
        DashboardPullRequestsState::EmptyVerified
    } else {
        DashboardPullRequestsState::Ready(filtered)
    }
}

pub(in crate::state::actions) fn dashboard_work_item_state(
    work_items: Vec<WorkItem>,
    current_user: &ExactUserIdentity,
    assigned_scoped_by_id: bool,
) -> DashboardWorkItemsState {
    if !current_user.is_known() {
        return DashboardWorkItemsState::Unavailable(
            DASHBOARD_WORK_ITEMS_IDENTITY_UNAVAILABLE_MESSAGE.to_string(),
        );
    }

    let mut filtered: Vec<WorkItem> = work_items
        .into_iter()
        .filter(|wi| match wi.fields.assigned_to.as_ref() {
            Some(AssignedToField::Identity(identity)) => {
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

    normalize_work_items(&mut filtered);

    if filtered.is_empty() {
        DashboardWorkItemsState::EmptyVerified
    } else {
        DashboardWorkItemsState::Ready(filtered)
    }
}

pub(in crate::state::actions) fn dashboard_pull_requests(
    app: &mut App,
    pull_requests: Vec<PullRequest>,
    creator_scoped_by_id: bool,
) {
    tracing::info!(count = pull_requests.len(), "dashboard PRs loaded");
    app.dashboard_pull_requests =
        dashboard_pull_request_state(pull_requests, &app.core.current_user, creator_scoped_by_id);
    app.rebuild_dashboard();
}

pub(in crate::state::actions) fn dashboard_pull_requests_failed(app: &mut App, message: String) {
    tracing::warn!(%message, "dashboard pull request fetch failed");
    app.notifications.error_dedup(message.clone());
    app.dashboard_pull_requests = app.dashboard_pull_requests.stale_or_unavailable(message);
    app.rebuild_dashboard();
}

pub(in crate::state::actions) fn dashboard_work_items(
    app: &mut App,
    work_items: Vec<WorkItem>,
    assigned_scoped_by_id: bool,
) {
    tracing::info!(count = work_items.len(), "dashboard work items loaded");
    app.dashboard_work_items =
        dashboard_work_item_state(work_items, &app.core.current_user, assigned_scoped_by_id);
    app.rebuild_dashboard();
}

pub(in crate::state::actions) fn dashboard_work_items_failed(app: &mut App, message: String) {
    tracing::warn!(%message, "dashboard work items fetch failed");
    app.notifications.error_dedup(message.clone());
    app.dashboard_work_items = app.dashboard_work_items.stale_or_unavailable(message);
    app.rebuild_dashboard();
}

pub(in crate::state::actions) fn pinned_work_items(app: &mut App, mut work_items: Vec<WorkItem>) {
    tracing::info!(count = work_items.len(), "pinned work items loaded");
    normalize_work_items(&mut work_items);
    app.pinned_work_items = PinnedWorkItemsState::Ready(work_items);
    app.rebuild_dashboard();
}

pub(in crate::state::actions) fn pinned_work_items_failed(app: &mut App, message: String) {
    tracing::warn!(%message, "pinned work items fetch failed");
    app.notifications.error_dedup(message.clone());
    app.pinned_work_items = app.pinned_work_items.stale_or_unavailable(message);
    app.rebuild_dashboard();
}

pub(in crate::state::actions) fn user_identity(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    identity: ExactUserIdentity,
) {
    tracing::info!("user identity resolved");
    app.core.current_user = identity;
    if app.view == View::Dashboard {
        // Re-fetch dashboard sections now that exact identity is available.
        spawn_fetch_dashboard_pull_requests(app, client, tx);
        spawn_fetch_dashboard_work_items(app, client, tx);
        spawn_fetch_pinned_work_items(app, client, tx);
    }
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

pub(in crate::state::actions) fn user_identity_failed(app: &mut App, message: String) {
    tracing::warn!(%message, "user identity resolution failed");
    app.notifications.error_dedup(message.clone());
    app.core.current_user = ExactUserIdentity::default();
    app.dashboard_pull_requests = DashboardPullRequestsState::Unavailable(message.clone());
    app.dashboard_work_items = DashboardWorkItemsState::Unavailable(message);
    app.rebuild_dashboard();
}

fn normalize_pull_requests(pull_requests: &mut Vec<PullRequest>) {
    let mut order = Vec::new();
    let mut by_id = std::collections::BTreeMap::new();
    for pull_request in std::mem::take(pull_requests) {
        if !by_id.contains_key(&pull_request.pull_request_id) {
            order.push(pull_request.pull_request_id);
        }
        by_id.insert(pull_request.pull_request_id, pull_request);
    }
    *pull_requests = order
        .iter()
        .filter_map(|pull_request_id| by_id.get(pull_request_id).cloned())
        .collect();
}

fn normalize_work_items(work_items: &mut Vec<WorkItem>) {
    let mut order = Vec::new();
    let mut by_id = std::collections::BTreeMap::new();
    for work_item in std::mem::take(work_items) {
        if !by_id.contains_key(&work_item.id) {
            order.push(work_item.id);
        }
        by_id.insert(work_item.id, work_item);
    }
    *work_items = order
        .iter()
        .filter_map(|work_item_id| by_id.get(work_item_id).cloned())
        .collect();
}
