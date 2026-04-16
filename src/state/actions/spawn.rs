//! Spawns async API calls and background tasks.

use std::future::Future;

use anyhow::Result;
use futures::future::join_all;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::client::http::AdoClient;
use crate::client::models::{BacklogLevelConfiguration, ProjectTeam, WorkItem};

use super::super::messages::{AppMessage, RefreshSource};
use super::super::{App, DashboardPullRequestsState, ExactUserIdentity};

const DASHBOARD_IDENTITY_UNAVAILABLE_MESSAGE: &str =
    "Unable to verify your Azure DevOps identity — My Pull Requests unavailable";
const DASHBOARD_PULL_REQUESTS_UNAVAILABLE_MESSAGE: &str = "Failed to load My Pull Requests";
const BOARDS_FETCH_FAILED_MESSAGE: &str = "Failed to load backlog";
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

fn collect_backlog_work_item_ids(work_items: &[crate::client::models::WorkItemLink]) -> Vec<u32> {
    let mut ids = std::collections::BTreeSet::new();
    for work_item in work_items {
        if let Some(source) = &work_item.source {
            ids.insert(source.id);
        }
        if let Some(target) = &work_item.target {
            ids.insert(target.id);
        }
    }
    ids.into_iter().collect()
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

    let backlog_results = join_all(backlogs.iter().map(|backlog| {
        let client = client.clone();
        let team_name = team_name.clone();
        let backlog_id = backlog.id.clone();
        async move {
            let work_items = client
                .list_backlog_level_work_items(&team_name, &backlog_id)
                .await?;
            Ok::<_, anyhow::Error>((backlog_id, work_items))
        }
    }))
    .await;

    let mut work_item_ids = std::collections::BTreeSet::new();
    for backlog_result in backlog_results {
        let (_, backlog_work_items) = backlog_result?;
        work_item_ids.extend(collect_backlog_work_item_ids(&backlog_work_items));
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
        tokio::spawn(
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
                            log_id,
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
                        let _ = tx
                            .send(AppMessage::RefreshError {
                                message: format!("Refresh timeline: {e}"),
                                source: RefreshSource::Log,
                            })
                            .await;
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

/// Spawns an async task that fetches pull requests from the Azure DevOps REST API.
pub fn spawn_fetch_pull_requests(
    app: &App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    generation: u64,
) {
    use crate::components::pull_requests::PrViewMode;

    let mode = app.pull_requests.mode;
    let user_id = app.current_user.id.clone();

    // Warn when a filtered mode cannot actually filter.
    if user_id.is_none() && matches!(mode, PrViewMode::CreatedByMe | PrViewMode::AssignedToMe) {
        tracing::warn!(
            ?mode,
            "user identity not resolved — PR filter will be unscoped"
        );
    }

    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("fetch_pull_requests", ?mode, generation);
    tokio::spawn(
        async move {
            let (status, creator_id, reviewer_id) = match mode {
                PrViewMode::CreatedByMe => ("active", user_id.as_deref(), None),
                PrViewMode::AssignedToMe => ("active", None, user_id.as_deref()),
                PrViewMode::AllActive => ("active", None, None),
            };
            let msg = match client
                .list_pull_requests(status, creator_id, reviewer_id)
                .await
            {
                Ok(prs) => AppMessage::PullRequestsLoaded {
                    pull_requests: prs,
                    generation,
                },
                Err(e) => AppMessage::Error(format!("Fetch pull requests: {e}")),
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
    tokio::spawn(
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
    tokio::spawn(
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
                (Err(e), _) | (_, Err(e)) => AppMessage::Error(format!("Fetch PR detail: {e}")),
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
    tokio::spawn(
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
    tokio::spawn(
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
    fn collect_backlog_work_item_ids_deduplicates_sources_and_targets() {
        let ids = collect_backlog_work_item_ids(&[
            crate::client::models::WorkItemLink {
                rel: None,
                source: Some(crate::client::models::WorkItemReference { id: 7, url: None }),
                target: Some(crate::client::models::WorkItemReference { id: 9, url: None }),
            },
            crate::client::models::WorkItemLink {
                rel: None,
                source: Some(crate::client::models::WorkItemReference { id: 7, url: None }),
                target: Some(crate::client::models::WorkItemReference { id: 11, url: None }),
            },
        ]);

        assert_eq!(ids, vec![7, 9, 11]);
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
}
