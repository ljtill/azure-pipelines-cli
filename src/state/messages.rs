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
}
