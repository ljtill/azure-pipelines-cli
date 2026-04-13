//! Spawns async API calls and background tasks.

use std::future::Future;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::client::http::AdoClient;

use super::super::App;
use super::super::messages::{AppMessage, RefreshSource};

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
pub fn spawn_fetch_pull_requests(app: &App, client: &AdoClient, tx: &mpsc::Sender<AppMessage>) {
    use crate::components::pull_requests::PrViewMode;

    let mode = app.pull_requests.mode;
    let user_id = app.user_id.clone();
    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("fetch_pull_requests", ?mode);
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
                Ok(prs) => AppMessage::PullRequestsLoaded { pull_requests: prs },
                Err(e) => AppMessage::Error(format!("Fetch pull requests: {e}")),
            };
            let _ = tx.send(msg).await;
        }
        .instrument(span),
    );
}

/// Spawns an async task that fetches "Created by me" PRs for the Dashboard.
pub fn spawn_fetch_dashboard_pull_requests(
    app: &App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
) {
    let user_id = app.user_id.clone();
    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("fetch_dashboard_prs");
    tokio::spawn(
        async move {
            let msg = match client
                .list_pull_requests("active", user_id.as_deref(), None)
                .await
            {
                Ok(prs) => AppMessage::DashboardPullRequests { pull_requests: prs },
                Err(e) => {
                    tracing::debug!(error = %e, "dashboard PR fetch failed (non-fatal)");
                    AppMessage::DashboardPullRequests {
                        pull_requests: vec![],
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
///
/// Sends `UserIdentity` on success or `UserIdentityFailed` on failure so the
/// dashboard can fall back to an unfiltered PR fetch.
pub fn spawn_fetch_user_identity(client: &AdoClient, tx: &mpsc::Sender<AppMessage>) {
    let client = client.clone();
    let tx = tx.clone();
    let span = tracing::info_span!("fetch_user_identity");
    tokio::spawn(
        async move {
            match client.get_connection_data().await {
                Ok(cd) => {
                    if let Some(id) = cd.user_id() {
                        let _ = tx
                            .send(AppMessage::UserIdentity {
                                user_id: id.to_string(),
                            })
                            .await;
                    } else {
                        tracing::warn!("connection data returned no user id");
                        let _ = tx.send(AppMessage::UserIdentityFailed).await;
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to resolve user identity");
                    let _ = tx.send(AppMessage::UserIdentityFailed).await;
                }
            }
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
