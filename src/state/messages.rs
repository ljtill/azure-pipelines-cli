//! Asynchronous message types for state updates.

use crate::client::models::{
    Approval, Build, BuildTimeline, PipelineDefinition, PullRequest, PullRequestThread,
    RetentionLease,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshSource {
    Data,
    BuildHistory,
    Approvals,
    Log,
}

/// Represents a message sent from background tasks to the main event loop.
pub enum AppMessage {
    DataRefresh {
        definitions: Vec<PipelineDefinition>,
        recent_builds: Vec<Build>,
        pending_approvals: Vec<Approval>,
        retention_leases: Vec<RetentionLease>,
    },
    BuildHistory {
        builds: Vec<Build>,
        continuation_token: Option<String>,
    },
    BuildHistoryMore {
        builds: Vec<Build>,
        continuation_token: Option<String>,
    },
    Timeline {
        build_id: u32,
        timeline: BuildTimeline,
        generation: u64,
        is_refresh: bool,
    },
    LogContent {
        content: String,
        generation: u64,
        log_id: u32,
    },
    LogRefreshFinished {
        had_failure: bool,
    },
    BuildCancelled,
    BuildsCancelled {
        cancelled: u32,
        failed: u32,
    },
    StageRetried,
    CheckUpdated,
    PipelineQueued {
        build: Build,
        #[allow(dead_code)]
        definition_id: u32,
    },
    UpdateAvailable {
        version: String,
    },
    Error(String),
    /// Behaves like `Error`, but for periodic refresh failures — uses dedup to avoid
    /// flooding the notification queue when the network is persistently down.
    RefreshError {
        message: String,
        source: RefreshSource,
    },
    RetentionLeasesDeleted {
        deleted: u32,
        failed: u32,
    },
    PullRequestsLoaded {
        pull_requests: Vec<PullRequest>,
    },
    UserIdentity {
        user_id: String,
    },
    PullRequestDetailLoaded {
        pull_request: PullRequest,
        threads: Vec<PullRequestThread>,
    },
    DashboardPullRequests {
        pull_requests: Vec<PullRequest>,
    },
}
