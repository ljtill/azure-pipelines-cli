use crate::api::models::{Approval, Build, BuildTimeline, PipelineDefinition, RetentionLease};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshSource {
    Data,
    BuildHistory,
    Approvals,
    Log,
}

/// Messages sent from background tasks to the main event loop.
pub enum AppMessage {
    DataRefresh {
        definitions: Vec<PipelineDefinition>,
        recent_builds: Vec<Build>,
        pending_approvals: Vec<Approval>,
        retention_leases: Vec<RetentionLease>,
    },
    BuildHistory {
        builds: Vec<Build>,
    },
    BuildHistoryMore {
        builds: Vec<Build>,
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
    /// Like `Error`, but for periodic refresh failures — uses dedup to avoid
    /// flooding the notification queue when the network is persistently down.
    RefreshError {
        message: String,
        source: RefreshSource,
    },
    RetentionLeasesDeleted {
        deleted: u32,
        failed: u32,
    },
}
