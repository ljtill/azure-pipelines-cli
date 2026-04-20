//! Asynchronous message types for state updates.

use crate::client::models::{
    Approval, BacklogLevelConfiguration, Build, BuildTimeline, PipelineDefinition, PullRequest,
    PullRequestThread, RetentionLease, WorkItem, WorkItemComment,
};

use super::ExactUserIdentity;

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
        generation: u64,
    },
    UserIdentity {
        identity: ExactUserIdentity,
    },
    PullRequestDetailLoaded {
        pull_request: PullRequest,
        threads: Vec<PullRequestThread>,
    },
    WorkItemDetailLoaded {
        work_item_id: u32,
        work_item: Box<WorkItem>,
        comments: Vec<WorkItemComment>,
    },
    WorkItemDetailFailed {
        work_item_id: u32,
        message: String,
    },
    DashboardPullRequests {
        pull_requests: Vec<PullRequest>,
        creator_scoped_by_id: bool,
    },
    DashboardPullRequestsFailed {
        message: String,
    },
    DashboardWorkItems {
        work_items: Vec<WorkItem>,
        assigned_scoped_by_id: bool,
    },
    DashboardWorkItemsFailed {
        message: String,
    },
    PinnedWorkItems {
        work_items: Vec<WorkItem>,
    },
    PinnedWorkItemsFailed {
        message: String,
    },
    UserIdentityFailed {
        message: String,
    },
    BoardsLoaded {
        team_name: String,
        backlogs: Vec<BacklogLevelConfiguration>,
        work_items: Vec<WorkItem>,
        generation: u64,
    },
    BoardsFailed {
        message: String,
        generation: u64,
    },
    MyWorkItemsLoaded {
        view: super::View,
        work_items: Vec<WorkItem>,
        generation: u64,
    },
    MyWorkItemsFailed {
        view: super::View,
        message: String,
        generation: u64,
    },
    /// Signals that a named background task panicked. The UI surfaces the
    /// failure so users are not left staring at a frozen view.
    TaskPanicked {
        task_name: &'static str,
        message: String,
    },
    /// Signals that Azure DevOps rejected the requested `api-version`. This is
    /// surfaced as a persistent error notification prompting the user to pass
    /// `--api-version` or set `DEVOPS_API_VERSION`.
    AdoApiVersionUnsupported {
        requested: String,
        server_message: String,
    },
    /// Reports per-page progress from a paginated fetcher so the UI can
    /// indicate forward motion during long list operations.
    PaginationProgress {
        endpoint: &'static str,
        page: usize,
        items: usize,
    },
}
