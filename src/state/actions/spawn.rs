//! Spawns async API calls and background tasks.

use std::future::Future;
use std::panic::AssertUnwindSafe;

use anyhow::Result;
use futures::FutureExt;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::client::http::{AdoClient, ApiVersionUnsupported, PaginationProgress};
use crate::client::models::{BacklogLevelConfiguration, ProjectTeam, WorkItem};
use crate::client::wiql::wiql_escape;

use super::super::messages::{AppMessage, RefreshSource};
use super::super::{
    App, DashboardPullRequestsState, DashboardWorkItemsState, ExactUserIdentity,
    PinnedWorkItemsState,
};

const DASHBOARD_IDENTITY_UNAVAILABLE_MESSAGE: &str =
    "Unable to verify your Azure DevOps identity — My Pull Requests unavailable";
const DASHBOARD_PULL_REQUESTS_UNAVAILABLE_MESSAGE: &str = "Failed to load My Pull Requests";
const DASHBOARD_WORK_ITEMS_UNAVAILABLE_MESSAGE: &str = "Failed to load My Work Items";
const PINNED_WORK_ITEMS_UNAVAILABLE_MESSAGE: &str = "Failed to load pinned work items";
const BOARDS_FETCH_FAILED_MESSAGE: &str = "Failed to load backlog";
const MY_WORK_ITEMS_FETCH_FAILED_MESSAGE: &str = "Failed to load work items";
const BOARD_FIELDS: &[&str] = &[
    "System.Title",
    "System.WorkItemType",
    "System.State",
    "System.AssignedTo",
    "System.IterationPath",
    "System.AreaPath",
    "System.Parent",
    "System.BoardColumn",
    "Microsoft.VSTS.Common.StackRank",
];

fn dashboard_identity_unavailable_message(detail: &str) -> String {
    format!("{DASHBOARD_IDENTITY_UNAVAILABLE_MESSAGE}: {detail}")
}

fn describe_connection_data_error(error: &anyhow::Error) -> String {
    let flattened_message = error
        .to_string()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if flattened_message.starts_with("Authentication failed") {
        return "authentication failed — run `az login` or `azd auth login`".to_string();
    }

    if let Some(reqwest_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<reqwest::Error>())
    {
        if reqwest_error.is_timeout() {
            return "connection data request timed out".to_string();
        }
        if reqwest_error.is_connect() {
            return "could not reach Azure DevOps".to_string();
        }
        if let Some(status) = reqwest_error.status() {
            return match status.as_u16() {
                401 => "connection data request was unauthorized (401)".to_string(),
                403 => "connection data request was forbidden (403)".to_string(),
                404 => "connection data endpoint was not found (404)".to_string(),
                code => format!("connection data request failed with HTTP {code}"),
            };
        }
    }

    format!("connection data request failed: {flattened_message}")
}

fn choose_boards_team<'a>(teams: &'a [ProjectTeam], project: &str) -> Option<&'a ProjectTeam> {
    teams
        .iter()
        .find(|team| team.is_default_project_team())
        .or_else(|| {
            let default_name = format!("{project} Team");
            teams
                .iter()
                .find(|team| team.name.eq_ignore_ascii_case(&default_name))
        })
        .or_else(|| teams.first())
}

/// Builds a WIQL query returning every Epic in the project, ordered by
/// stack rank then id. Used as the root seed for the Boards hierarchy so the
/// tree covers the entire project, not just one team's configured backlog.
pub(crate) fn build_board_epic_roots_wiql(project: &str) -> String {
    let escaped = wiql_escape(project);
    format!(
        "SELECT [System.Id] FROM WorkItems \
WHERE [System.TeamProject] = '{escaped}' \
AND [System.WorkItemType] = 'Epic' \
ORDER BY [Microsoft.VSTS.Common.StackRank], [System.Id]"
    )
}

/// Builds a recursive `WorkItemLinks` WIQL query that returns every
/// `Hierarchy-Forward` (parent → child) link in the project whose target is
/// not in a terminal state. Used to discover descendants (Tasks, Bugs, Test
/// Cases, etc.) below the Epic roots.
pub(crate) fn build_board_descendants_wiql(project: &str) -> String {
    let escaped = wiql_escape(project);
    format!(
        "SELECT [System.Id] FROM WorkItemLinks \
WHERE [Source].[System.TeamProject] = '{escaped}' \
AND [Target].[System.TeamProject] = '{escaped}' \
AND [Target].[System.State] NOT IN ('Closed', 'Removed', 'Done', 'Cut') \
AND [System.Links.LinkType] = 'System.LinkTypes.Hierarchy-Forward' \
MODE (Recursive)"
    )
}

/// Returns the set of work item IDs transitively reachable from the `seeds`
/// via the supplied `(source, target)` hierarchy links. Safe against cycles.
pub(crate) fn hierarchy_descendant_ids(
    seeds: &[u32],
    links: &[crate::client::models::WorkItemLink],
) -> std::collections::BTreeSet<u32> {
    use std::collections::{BTreeSet, HashMap};
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for link in links {
        if let (Some(src), Some(tgt)) = (link.source.as_ref(), link.target.as_ref()) {
            children.entry(src.id).or_default().push(tgt.id);
        }
    }

    let mut visited: BTreeSet<u32> = BTreeSet::new();
    let mut stack: Vec<u32> = seeds.to_vec();
    while let Some(id) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        if let Some(kids) = children.get(&id) {
            stack.extend(kids.iter().copied());
        }
    }
    visited
}

async fn load_boards_snapshot(
    client: &AdoClient,
    project: &str,
) -> Result<(String, Vec<BacklogLevelConfiguration>, Vec<WorkItem>)> {
    let teams = client.list_project_teams().await?;
    let team = choose_boards_team(&teams, project)
        .ok_or_else(|| anyhow::anyhow!("Azure DevOps returned no teams for project `{project}`"))?;
    let team_name = team.name.clone();

    let mut backlogs = client.list_backlogs(&team_name).await?;
    backlogs.sort_by_key(|backlog| backlog.rank);

    // Seed the tree with every Epic in the project so the backlog view is not
    // constrained to a single team's backlog configuration. Descendants
    // (Features, Stories, Tasks, Bugs, etc.) are pulled in via the recursive
    // Hierarchy-Forward query below.
    let epic_wiql = build_board_epic_roots_wiql(project);
    let epic_query = client.query_by_wiql(&epic_wiql).await?;
    let mut work_item_ids: std::collections::BTreeSet<u32> = epic_query
        .work_items
        .iter()
        .map(|reference| reference.id)
        .collect();

    // Discover descendants (e.g. Tasks) below the Epic roots by running a
    // recursive Hierarchy-Forward WorkItemLinks query and restricting results
    // to the transitive closure of the Epic seeds.
    let seed_ids: Vec<u32> = work_item_ids.iter().copied().collect();
    if !seed_ids.is_empty() {
        let wiql = build_board_descendants_wiql(project);
        match client.query_by_wiql(&wiql).await {
            Ok(result) => {
                let reachable = hierarchy_descendant_ids(&seed_ids, &result.work_item_relations);
                work_item_ids.extend(reachable);
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    "failed to fetch board descendants; rendering Epics only"
                );
            }
        }
    }

    let work_items = client
        .get_work_items_batch(
            &work_item_ids.into_iter().collect::<Vec<_>>(),
            BOARD_FIELDS,
            None,
        )
        .await?;

    Ok((team_name, backlogs, work_items))
}

// --- Drop guard for async refresh tasks ---

/// Ensures a fallback message is sent if the spawned task exits unexpectedly
/// (e.g., due to a panic). Call `defuse()` on the happy path to suppress.
pub(super) struct RefreshGuard {
    tx: Option<mpsc::Sender<AppMessage>>,
    fallback: Option<AppMessage>,
}

impl RefreshGuard {
    pub(super) fn new(tx: mpsc::Sender<AppMessage>, fallback: AppMessage) -> Self {
        Self {
            tx: Some(tx),
            fallback: Some(fallback),
        }
    }

    /// Disarms the guard — no fallback message will be sent on drop.
    pub(super) fn defuse(&mut self) {
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

/// Returns an `AppMessage::AdoApiVersionUnsupported` when the error chain
/// contains a typed [`ApiVersionUnsupported`], otherwise `None`.
///
/// Error conversion sites in this module consult this helper first so users
/// get an actionable notification instead of a generic "request failed".
pub(super) fn api_version_unsupported_message(e: &anyhow::Error) -> Option<AppMessage> {
    e.chain()
        .find_map(|cause| cause.downcast_ref::<ApiVersionUnsupported>())
        .map(|v| AppMessage::AdoApiVersionUnsupported {
            requested: v.requested.clone(),
            server_message: v.server_message.clone(),
        })
}

/// Builds an `AppMessage` for an API error, preferring the typed
/// `AdoApiVersionUnsupported` variant when applicable and falling back to the
/// caller-supplied generic message factory otherwise.
pub(super) fn error_to_message(
    e: anyhow::Error,
    generic: impl FnOnce(anyhow::Error) -> AppMessage,
) -> AppMessage {
    if let Some(msg) = api_version_unsupported_message(&e) {
        return msg;
    }
    generic(e)
}

/// Spawns a named tokio task. If the task panics, a `TaskPanicked` message is
/// sent through `tx` so the UI can surface the failure instead of leaving the
/// user with a frozen or stale view.
pub(super) fn spawn_named<F>(task_name: &'static str, tx: mpsc::Sender<AppMessage>, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let tx_panic = tx.clone();
        let result = AssertUnwindSafe(fut).catch_unwind().await;
        if let Err(panic_payload) = result {
            let message = panic_message(&panic_payload);
            tracing::error!(task_name, %message, "background task panicked");
            let _ = tx_panic
                .send(AppMessage::TaskPanicked { task_name, message })
                .await;
        }
    });
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&'static str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic payload".to_string())
}

/// Spawns an async API call on a background task, routing the result to AppMessage.
pub(super) fn spawn_api<F, Fut, T>(
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
    spawn_named(
        context,
        tx.clone(),
        async move {
            let msg = match call(client).await {
                Ok(val) => on_ok(val),
                Err(e) => error_to_message(e, |e| AppMessage::Error(format!("{context}: {e}"))),
            };
            let _ = tx.send(msg).await;
        }
        .instrument(span),
    );
}

pub fn spawn_data_refresh(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
) -> bool {
    if !app.data_refresh.start() {
        return false;
    }

    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("data_refresh");
    spawn_named(
        "data_refresh",
        tx.clone(),
        async move {
            let mut guard = RefreshGuard::new(
                tx.clone(),
                AppMessage::RefreshError {
                    message: "Data refresh task terminated unexpectedly".into(),
                    source: RefreshSource::Data,
                },
            );

            // Progress callback shared across paginated fetchers during this
            // refresh. `try_send` is non-blocking so a full channel simply
            // drops progress events — losing one update is harmless.
            let progress_tx = tx.clone();
            let progress = move |p: PaginationProgress| {
                let _ = progress_tx.try_send(AppMessage::PaginationProgress {
                    endpoint: p.endpoint,
                    page: p.page,
                    items: p.items_so_far,
                });
            };

            let (defs_result, recent_result, approvals_result) = tokio::join!(
                client.list_definitions_with_progress(Some(&progress)),
                client.list_recent_builds(),
                client.list_pending_approvals(),
            );

            let pending_approvals = match approvals_result {
                Ok(approvals) => approvals,
                Err(e) => {
                    let msg = error_to_message(e, |e| AppMessage::RefreshError {
                        message: format!("Approvals unavailable: {e}"),
                        source: RefreshSource::Approvals,
                    });
                    let _ = tx.send(msg).await;
                    Vec::new()
                }
            };

            match (defs_result, recent_result) {
                (Ok(definitions), Ok(recent_builds)) => {
                    // Fetch retention leases in parallel across all definitions.
                    // Done after definitions are known so we have the IDs.
                    let def_ids: Vec<u32> = definitions.iter().map(|d| d.id).collect();
                    let retention_leases = match client.list_all_retention_leases(&def_ids).await {
                        Ok(leases) => leases,
                        Err(e) => {
                            tracing::warn!(error = %e, "retention leases unavailable");
                            Vec::new()
                        }
                    };

                    let _ = tx
                        .send(AppMessage::DataRefresh {
                            definitions,
                            recent_builds,
                            pending_approvals,
                            retention_leases,
                        })
                        .await;
                }
                (Err(e), _) | (_, Err(e)) => {
                    let msg = error_to_message(e, |e| AppMessage::RefreshError {
                        message: format!("Refresh: {e}"),
                        source: RefreshSource::Data,
                    });
                    let _ = tx.send(msg).await;
                }
            }

            guard.defuse();
        }
        .instrument(span),
    );
    true
}

/// Re-fetches the build history for the currently selected pipeline definition.
///
/// When `top` is `Some(n)`, request up to `n` builds in a single page instead
/// of the default `TOP_DEFINITION_BUILDS` (20). This is used after in-place
/// refreshes (e.g. lease deletion) so the response covers all previously loaded
/// builds and the scroll position can be restored.
pub(super) fn spawn_build_history_refresh(
    app: &App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    top: Option<u32>,
) {
    if let Some(def) = &app.build_history.selected_definition {
        let client = client.clone();
        let tx = tx.clone();
        let def_id = def.id;
        let span = tracing::debug_span!("build_history_refresh", definition_id = def_id);
        spawn_named(
            "build_history_refresh",
            tx.clone(),
            async move {
                let result = match top {
                    Some(n) => client.list_builds_for_definition_top(def_id, n).await,
                    None => client.list_builds_for_definition(def_id).await,
                };
                match result {
                    Ok((builds, continuation_token)) => {
                        let _ = tx
                            .send(AppMessage::BuildHistory {
                                builds,
                                continuation_token,
                            })
                            .await;
                    }
                    Err(e) => {
                        let msg = error_to_message(e, |e| AppMessage::RefreshError {
                            message: format!("Refresh builds: {e}"),
                            source: RefreshSource::BuildHistory,
                        });
                        let _ = tx.send(msg).await;
                    }
                }
            }
            .instrument(span),
        );
    }
}

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
    spawn_named(
        "log_fetch",
        tx.clone(),
        async move {
            match client.get_build_log(build_id, log_id).await {
                Ok(content) => {
                    let _ = tx
                        .send(AppMessage::LogContent {
                            content,
                            generation,
                            log_id,
                        })
                        .await;
                }
                Err(e) => {
                    let msg = error_to_message(e, |e| AppMessage::Error(format!("Fetch log: {e}")));
                    let _ = tx.send(msg).await;
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
    spawn_named(
        "timeline_fetch",
        tx.clone(),
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
                    let msg =
                        error_to_message(e, |e| AppMessage::Error(format!("Fetch timeline: {e}")));
                    let _ = tx.send(msg).await;
                }
            }
        }
        .instrument(span),
    );
}

pub fn spawn_log_refresh(app: &mut App, client: &AdoClient, tx: &mpsc::Sender<AppMessage>) -> bool {
    if !app.log_refresh.start() {
        return false;
    }
    let generation = app.log_viewer.generation();
    let Some(build) = app.log_viewer.selected_build() else {
        app.log_refresh.succeed(); // wasn't really in-flight
        return false;
    };
    let build_id = build.id;
    let should_refresh_timeline = build.status.is_in_progress();
    let log_id_to_refresh = if app.log_viewer.is_following() {
        app.log_viewer.followed_log_id()
    } else {
        app.log_viewer
            .timeline_task_log_id(app.log_viewer.nav().index())
    };
    let should_refresh_log = log_id_to_refresh.is_some()
        && (!app.log_viewer.log_content().is_empty() || should_refresh_timeline);

    let timeline_client = client.clone();
    let log_client = client.clone();
    let tx = tx.clone();
    let span = tracing::debug_span!("log_refresh", build_id);
    spawn_named(
        "log_refresh",
        tx.clone(),
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
                        Some((log_id, log_client.get_build_log(build_id, log_id).await))
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
                        let msg = error_to_message(e, |e| AppMessage::RefreshError {
                            message: format!("Refresh timeline: {e}"),
                            source: RefreshSource::Log,
                        });
                        let _ = tx.send(msg).await;
                    }
                }
            }

            if let Some((log_id, result)) = log_result {
                match result {
                    Ok(content) => {
                        let _ = tx
                            .send(AppMessage::LogContent {
                                content,
                                generation,
                                log_id,
                            })
                            .await;
                    }
                    Err(e) => {
                        had_failure = true;
                        let msg = error_to_message(e, |e| AppMessage::RefreshError {
                            message: format!("Refresh log: {e}"),
                            source: RefreshSource::Log,
                        });
                        let _ = tx.send(msg).await;
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

/// Spawns an async task that fetches pull requests from the Azure DevOps REST API.
pub fn spawn_fetch_pull_requests(
    app: &App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    generation: u64,
) {
    use crate::state::View;

    let view = app.view;
    let user_id = app.current_user.id.clone();

    // Warn when a filtered view cannot actually filter.
    if user_id.is_none()
        && matches!(
            view,
            View::PullRequestsCreatedByMe | View::PullRequestsAssignedToMe
        )
    {
        tracing::warn!(
            ?view,
            "user identity not resolved — PR filter will be unscoped"
        );
    }

    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("fetch_pull_requests", ?view, generation);
    spawn_named(
        "fetch_pull_requests",
        tx.clone(),
        async move {
            let (status, creator_id, reviewer_id) = match view {
                View::PullRequestsAssignedToMe => ("active", None, user_id.as_deref()),
                View::PullRequestsAllActive => ("active", None, None),
                // Default to CreatedByMe semantics for the root PR view.
                _ => ("active", user_id.as_deref(), None),
            };
            let msg = match client
                .list_pull_requests(status, creator_id, reviewer_id)
                .await
            {
                Ok(prs) => AppMessage::PullRequestsLoaded {
                    pull_requests: prs,
                    generation,
                },
                Err(e) => error_to_message(e, |e| {
                    AppMessage::Error(format!("Fetch pull requests: {e}"))
                }),
            };
            let _ = tx.send(msg).await;
        }
        .instrument(span),
    );
}

/// Spawns an async task that fetches dashboard PRs once exact identity is known.
///
/// If identity is not yet available, retries identity resolution instead of
/// fetching unverifiable PR data.
pub fn spawn_fetch_dashboard_pull_requests(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
) {
    if !app.current_user.is_known() {
        if matches!(
            app.dashboard_pull_requests,
            DashboardPullRequestsState::Loading
        ) {
            return;
        }
        app.dashboard_pull_requests = DashboardPullRequestsState::Loading;
        app.rebuild_dashboard();
        spawn_fetch_user_identity(client, tx);
        return;
    }

    app.dashboard_pull_requests = DashboardPullRequestsState::Loading;
    app.rebuild_dashboard();

    let creator_id = app.current_user.id.clone();
    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("fetch_dashboard_prs");
    spawn_named(
        "fetch_dashboard_prs",
        tx.clone(),
        async move {
            let msg = match client
                .list_pull_requests("active", creator_id.as_deref(), None)
                .await
            {
                Ok(prs) => AppMessage::DashboardPullRequests {
                    pull_requests: prs,
                    creator_scoped_by_id: creator_id.is_some(),
                },
                Err(e) => {
                    tracing::debug!(error = %e, "dashboard PR fetch failed (non-fatal)");
                    AppMessage::DashboardPullRequestsFailed {
                        message: format!("{DASHBOARD_PULL_REQUESTS_UNAVAILABLE_MESSAGE}: {e}"),
                    }
                }
            };
            let _ = tx.send(msg).await;
        }
        .instrument(span),
    );
}

/// Returns the WIQL used to fetch dashboard work items (assigned to the current
/// user, active states, ordered by most recently changed).
pub(crate) fn build_dashboard_work_items_wiql(project: &str) -> String {
    let escaped_project = wiql_escape(project);
    format!(
        "SELECT [System.Id] FROM WorkItems \
         WHERE [System.AssignedTo] = @Me \
         AND [System.TeamProject] = '{escaped_project}' \
         AND [System.State] NOT IN ('Closed', 'Removed', 'Done', 'Cut') \
         ORDER BY [System.ChangedDate] DESC"
    )
}

/// Spawns an async task that fetches dashboard work items once exact identity is known.
///
/// If identity is not yet available, retries identity resolution instead of
/// fetching unverifiable assignee data.
pub fn spawn_fetch_dashboard_work_items(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
) {
    if !app.current_user.is_known() {
        if matches!(app.dashboard_work_items, DashboardWorkItemsState::Loading) {
            return;
        }
        app.dashboard_work_items = DashboardWorkItemsState::Loading;
        app.rebuild_dashboard();
        spawn_fetch_user_identity(client, tx);
        return;
    }

    app.dashboard_work_items = DashboardWorkItemsState::Loading;
    app.rebuild_dashboard();

    let wiql = build_dashboard_work_items_wiql(&app.current_config().azure_devops.project);
    let assigned_scoped_by_id = app.current_user.id.is_some();
    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("fetch_dashboard_work_items");
    spawn_named(
        "fetch_dashboard_work_items",
        tx.clone(),
        async move {
            let msg = match load_my_work_items(&client, &wiql).await {
                Ok(work_items) => AppMessage::DashboardWorkItems {
                    work_items,
                    assigned_scoped_by_id,
                },
                Err(e) => {
                    tracing::debug!(error = %e, "dashboard work items fetch failed (non-fatal)");
                    AppMessage::DashboardWorkItemsFailed {
                        message: format!("{DASHBOARD_WORK_ITEMS_UNAVAILABLE_MESSAGE}: {e}"),
                    }
                }
            };
            let _ = tx.send(msg).await;
        }
        .instrument(span),
    );
}

/// Spawns an async task that fetches the user's pinned work items by ID.
///
/// Short-circuits when no pins are configured, emitting an empty `Ready` state.
pub fn spawn_fetch_pinned_work_items(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
) {
    let ids: Vec<u32> = app.filters.pinned_work_item_ids.clone();
    if ids.is_empty() {
        app.pinned_work_items = PinnedWorkItemsState::Ready(Vec::new());
        app.rebuild_dashboard();
        return;
    }

    app.pinned_work_items = PinnedWorkItemsState::Loading;
    app.rebuild_dashboard();

    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("fetch_pinned_work_items", id_count = ids.len());
    spawn_named(
        "fetch_pinned_work_items",
        tx.clone(),
        async move {
            let fields = &[
                "System.Title",
                "System.WorkItemType",
                "System.State",
                "System.AssignedTo",
                "System.IterationPath",
            ];
            let msg = match client.get_work_items_batch(&ids, fields, None).await {
                Ok(items) => {
                    let mut by_id: std::collections::HashMap<u32, WorkItem> =
                        items.into_iter().map(|w| (w.id, w)).collect();
                    let ordered: Vec<WorkItem> =
                        ids.into_iter().filter_map(|id| by_id.remove(&id)).collect();
                    AppMessage::PinnedWorkItems {
                        work_items: ordered,
                    }
                }
                Err(e) => {
                    tracing::debug!(error = %e, "pinned work items fetch failed (non-fatal)");
                    AppMessage::PinnedWorkItemsFailed {
                        message: format!("{PINNED_WORK_ITEMS_UNAVAILABLE_MESSAGE}: {e}"),
                    }
                }
            };
            let _ = tx.send(msg).await;
        }
        .instrument(span),
    );
}

/// Spawns an async task that fetches a pull request and its threads in parallel.
pub fn spawn_fetch_pr_detail(
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    repo_id: String,
    pr_id: u32,
) {
    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("fetch_pr_detail", pr_id);
    spawn_named(
        "fetch_pr_detail",
        tx.clone(),
        async move {
            let (pr_result, threads_result) = tokio::join!(
                client.get_pull_request(&repo_id, pr_id),
                client.list_pull_request_threads(&repo_id, pr_id),
            );
            let msg = match (pr_result, threads_result) {
                (Ok(pull_request), Ok(threads)) => AppMessage::PullRequestDetailLoaded {
                    pull_request,
                    threads,
                },
                (Err(e), _) | (_, Err(e)) => {
                    error_to_message(e, |e| AppMessage::Error(format!("Fetch PR detail: {e}")))
                }
            };
            let _ = tx.send(msg).await;
        }
        .instrument(span),
    );
}

/// Spawns an async task that fetches a single work item (with relations) and
/// its comments in parallel.
pub fn spawn_fetch_work_item_detail(
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    work_item_id: u32,
) {
    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("fetch_work_item_detail", work_item_id);
    spawn_named(
        "fetch_work_item_detail",
        tx.clone(),
        async move {
            let (wi_result, comments_result) = tokio::join!(
                client.get_work_item_detail(work_item_id),
                client.list_work_item_comments(work_item_id),
            );
            let msg = match (wi_result, comments_result) {
                (Ok(work_item), Ok(comments)) => AppMessage::WorkItemDetailLoaded {
                    work_item_id,
                    work_item: Box::new(work_item),
                    comments,
                },
                (Err(e), _) | (_, Err(e)) => AppMessage::WorkItemDetailFailed {
                    work_item_id,
                    message: format!("Fetch work item detail: {e}"),
                },
            };
            let _ = tx.send(msg).await;
        }
        .instrument(span),
    );
}

/// Spawns a one-shot task to resolve the current user's identity from the ADO Connection Data API.
pub fn spawn_fetch_user_identity(client: &AdoClient, tx: &mpsc::Sender<AppMessage>) {
    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("fetch_user_identity");
    spawn_named(
        "fetch_user_identity",
        tx.clone(),
        async move {
            match client.get_connection_data().await {
                Ok(cd) => {
                    if let Some(user) = cd.authenticated_user {
                        let identity = ExactUserIdentity {
                            id: user.id,
                            unique_name: user.unique_name,
                            descriptor: user.descriptor,
                        };
                        if identity.is_known() {
                            let _ = tx.send(AppMessage::UserIdentity { identity }).await;
                        } else {
                            tracing::warn!("connection data returned no exact identity fields");
                            let _ = tx
                                .send(AppMessage::UserIdentityFailed {
                                    message: dashboard_identity_unavailable_message(
                                        "Azure DevOps did not return id, uniqueName, or descriptor for the signed-in user",
                                    ),
                                })
                                .await;
                        }
                    } else {
                        tracing::warn!("connection data returned no authenticated user");
                        let _ = tx
                            .send(AppMessage::UserIdentityFailed {
                                message: dashboard_identity_unavailable_message(
                                    "Azure DevOps connection data did not include an authenticated user",
                                ),
                            })
                            .await;
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to resolve user identity");
                    let _ = tx
                        .send(AppMessage::UserIdentityFailed {
                            message: dashboard_identity_unavailable_message(
                                &describe_connection_data_error(&e),
                            ),
                        })
                        .await;
                }
            }
        }
        .instrument(span),
    );
}

/// Spawns a one-shot task that loads the configured project's backlog tree snapshot.
pub fn spawn_fetch_boards(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    generation: u64,
) {
    app.boards.start_loading();

    let client = client.clone();
    let tx = tx.clone();
    let project = app.current_config().azure_devops.project;
    let span = tracing::info_span!("fetch_boards", generation, project = %project);
    spawn_named(
        "fetch_boards",
        tx.clone(),
        async move {
            let message = match load_boards_snapshot(&client, &project).await {
                Ok((team_name, backlogs, work_items)) => AppMessage::BoardsLoaded {
                    team_name,
                    backlogs,
                    work_items,
                    generation,
                },
                Err(error) => AppMessage::BoardsFailed {
                    message: format!("{BOARDS_FETCH_FAILED_MESSAGE}: {error}"),
                    generation,
                },
            };

            let _ = tx.send(message).await;
        }
        .instrument(span),
    );
}

/// Builds the WIQL for one of the personal Boards sub-views.
///
/// Returns `None` when the view is not a personal Boards sub-view. The query
/// uses the `@Me` token so it does not depend on identity resolution.
pub(crate) fn build_my_work_items_wiql(view: super::super::View, project: &str) -> Option<String> {
    let user_clause = match view {
        super::super::View::BoardsAssignedToMe => "[System.AssignedTo] = @Me",
        super::super::View::BoardsCreatedByMe => "[System.CreatedBy] = @Me",
        _ => return None,
    };
    // Escape single quotes in project name to avoid breaking out of the literal.
    let escaped_project = wiql_escape(project);
    Some(format!(
        "SELECT [System.Id] FROM WorkItems WHERE {user_clause} \
         AND [System.TeamProject] = '{escaped_project}' \
         AND [System.State] NOT IN ('Closed', 'Removed', 'Done', 'Cut') \
         ORDER BY [System.ChangedDate] DESC"
    ))
}

/// Spawns a one-shot task that loads the current user's personal work items
/// for the given view ("Assigned to me" or "Created by me").
pub fn spawn_fetch_my_work_items(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    view: super::super::View,
) {
    let Some(list) = app.my_work_items.list_for_mut(view) else {
        return;
    };
    let generation = list.next_generation();

    let Some(wiql) = build_my_work_items_wiql(view, &app.current_config().azure_devops.project)
    else {
        return;
    };

    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("fetch_my_work_items", ?view, generation);
    spawn_named(
        "fetch_my_work_items",
        tx.clone(),
        async move {
            let message = match load_my_work_items(&client, &wiql).await {
                Ok(work_items) => AppMessage::MyWorkItemsLoaded {
                    view,
                    work_items,
                    generation,
                },
                Err(error) => AppMessage::MyWorkItemsFailed {
                    view,
                    message: format!("{MY_WORK_ITEMS_FETCH_FAILED_MESSAGE}: {error}"),
                    generation,
                },
            };
            let _ = tx.send(message).await;
        }
        .instrument(span),
    );
}

async fn load_my_work_items(client: &AdoClient, wiql: &str) -> Result<Vec<WorkItem>> {
    let result = client.query_by_wiql(wiql).await?;
    let ids: Vec<u32> = result.work_items.iter().map(|r| r.id).collect();
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let fields = &[
        "System.Title",
        "System.WorkItemType",
        "System.State",
        "System.AssignedTo",
        "System.IterationPath",
    ];
    let items = client.get_work_items_batch(&ids, fields, None).await?;
    // Batch does not guarantee ordering; reorder to match the WIQL ordering.
    let mut by_id: std::collections::HashMap<u32, WorkItem> =
        items.into_iter().map(|w| (w.id, w)).collect();
    let ordered: Vec<WorkItem> = ids.into_iter().filter_map(|id| by_id.remove(&id)).collect();
    Ok(ordered)
}

/// Opens a URL in the platform's default browser.
pub(super) fn open_url(url: &str) -> std::io::Result<std::process::Child> {
    // Only allow https:// URLs to prevent command injection.
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

    #[tokio::test]
    async fn spawn_named_forwards_panic_as_task_panicked_message() {
        let (tx, mut rx) = mpsc::channel::<AppMessage>(4);
        spawn_named("test_task", tx, async {
            panic!("boom");
        });

        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timed out waiting for TaskPanicked message")
            .expect("channel closed unexpectedly");

        match msg {
            AppMessage::TaskPanicked { task_name, message } => {
                assert_eq!(task_name, "test_task");
                assert!(
                    message.contains("boom"),
                    "expected panic message to contain 'boom', got {message:?}"
                );
            }
            _ => panic!("unexpected message variant — expected TaskPanicked"),
        }
    }

    #[test]
    fn choose_boards_team_prefers_default_project_team_then_project_named_team() {
        let named_team = ProjectTeam {
            id: "named".to_string(),
            name: "Project Team".to_string(),
            description: None,
            project_id: None,
            project_name: None,
            url: None,
        };
        let default_team = ProjectTeam {
            id: "default".to_string(),
            name: "Delivery".to_string(),
            description: Some("The default project team.".to_string()),
            project_id: None,
            project_name: None,
            url: None,
        };

        let teams = [named_team.clone(), default_team.clone()];
        let chosen = choose_boards_team(&teams, "Project").unwrap();
        assert_eq!(chosen.id, default_team.id);

        let chosen = choose_boards_team(std::slice::from_ref(&named_team), "Project").unwrap();
        assert_eq!(chosen.id, named_team.id);
    }

    #[test]
    fn describe_connection_data_error_shortens_auth_failures() {
        let error = anyhow::anyhow!(
            "Authentication failed — ensure you are logged in with `az login` or `azd auth login`.\n\nUnderlying error: boom"
        );

        assert_eq!(
            describe_connection_data_error(&error),
            "authentication failed — run `az login` or `azd auth login`"
        );
    }

    #[test]
    fn describe_connection_data_error_flattens_generic_errors() {
        let error = anyhow::anyhow!("connection data blew up\nwith extra whitespace");

        assert_eq!(
            describe_connection_data_error(&error),
            "connection data request failed: connection data blew up with extra whitespace"
        );
    }

    #[test]
    fn build_my_work_items_wiql_for_assigned() {
        let wiql =
            build_my_work_items_wiql(super::super::super::View::BoardsAssignedToMe, "MyProject")
                .expect("wiql for assigned view");
        assert!(wiql.contains("[System.AssignedTo] = @Me"));
        assert!(wiql.contains("[System.TeamProject] = 'MyProject'"));
        assert!(wiql.contains("NOT IN ('Closed', 'Removed', 'Done', 'Cut')"));
        assert!(wiql.contains("ORDER BY [System.ChangedDate] DESC"));
    }

    #[test]
    fn build_my_work_items_wiql_for_created() {
        let wiql =
            build_my_work_items_wiql(super::super::super::View::BoardsCreatedByMe, "MyProject")
                .expect("wiql for created view");
        assert!(wiql.contains("[System.CreatedBy] = @Me"));
    }

    #[test]
    fn build_my_work_items_wiql_escapes_single_quotes_in_project() {
        let wiql =
            build_my_work_items_wiql(super::super::super::View::BoardsAssignedToMe, "it's mine")
                .expect("wiql");
        assert!(wiql.contains("'it''s mine'"));
    }

    #[test]
    fn build_my_work_items_wiql_rejects_non_boards_views() {
        assert!(
            build_my_work_items_wiql(super::super::super::View::Dashboard, "MyProject").is_none()
        );
        assert!(build_my_work_items_wiql(super::super::super::View::Boards, "MyProject").is_none());
    }

    #[test]
    fn build_board_descendants_wiql_scopes_recursively_to_project_and_excludes_terminal_states() {
        let wiql = build_board_descendants_wiql("MyProject");
        assert!(wiql.contains("FROM WorkItemLinks"));
        assert!(wiql.contains("[Source].[System.TeamProject] = 'MyProject'"));
        assert!(wiql.contains("[Target].[System.TeamProject] = 'MyProject'"));
        assert!(
            wiql.contains("[Target].[System.State] NOT IN ('Closed', 'Removed', 'Done', 'Cut')")
        );
        assert!(wiql.contains("'System.LinkTypes.Hierarchy-Forward'"));
        assert!(wiql.contains("MODE (Recursive)"));
    }

    #[test]
    fn build_board_descendants_wiql_escapes_single_quotes_in_project() {
        let wiql = build_board_descendants_wiql("it's mine");
        assert!(wiql.contains("'it''s mine'"));
    }

    #[test]
    fn build_board_epic_roots_wiql_selects_epics_scoped_to_project() {
        let wiql = build_board_epic_roots_wiql("MyProject");
        assert!(wiql.contains("FROM WorkItems"));
        assert!(wiql.contains("[System.TeamProject] = 'MyProject'"));
        assert!(wiql.contains("[System.WorkItemType] = 'Epic'"));
        assert!(wiql.contains("ORDER BY [Microsoft.VSTS.Common.StackRank], [System.Id]"));
    }

    #[test]
    fn build_board_epic_roots_wiql_escapes_single_quotes_in_project() {
        let wiql = build_board_epic_roots_wiql("it's mine");
        assert!(wiql.contains("'it''s mine'"));
    }

    fn link(source: Option<u32>, target: Option<u32>) -> crate::client::models::WorkItemLink {
        crate::client::models::WorkItemLink {
            rel: Some("System.LinkTypes.Hierarchy-Forward".to_string()),
            source: source.map(|id| crate::client::models::WorkItemReference { id, url: None }),
            target: target.map(|id| crate::client::models::WorkItemReference { id, url: None }),
        }
    }

    #[test]
    fn hierarchy_descendant_ids_walks_multi_level_tree_from_seeds() {
        // 1 -> 2 -> 3, 1 -> 4, 5 -> 6 (disjoint), 7 (orphan seed).
        let links = vec![
            link(Some(1), Some(2)),
            link(Some(2), Some(3)),
            link(Some(1), Some(4)),
            link(Some(5), Some(6)),
        ];
        let got = hierarchy_descendant_ids(&[1, 7], &links);
        assert!(got.contains(&1) && got.contains(&2) && got.contains(&3) && got.contains(&4));
        assert!(got.contains(&7));
        assert!(!got.contains(&5) && !got.contains(&6));
    }

    #[test]
    fn hierarchy_descendant_ids_is_safe_against_cycles() {
        // 1 -> 2 -> 1 (cycle), with an off-cycle child 2 -> 3.
        let links = vec![
            link(Some(1), Some(2)),
            link(Some(2), Some(1)),
            link(Some(2), Some(3)),
        ];
        let got = hierarchy_descendant_ids(&[1], &links);
        assert!(got.contains(&1) && got.contains(&2) && got.contains(&3));
    }

    #[test]
    fn hierarchy_descendant_ids_ignores_links_with_missing_endpoints() {
        let links = vec![
            link(None, Some(2)),
            link(Some(1), None),
            link(Some(1), Some(2)),
        ];
        let got = hierarchy_descendant_ids(&[1], &links);
        assert!(got.contains(&1) && got.contains(&2));
    }
}
