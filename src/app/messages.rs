use crate::api::models::{Approval, Build, BuildTimeline, PipelineDefinition};

/// Messages sent from background tasks to the main event loop.
pub enum AppMessage {
    DataRefresh {
        definitions: Vec<PipelineDefinition>,
        recent_builds: Vec<Build>,
        active_builds: Vec<Build>,
        pending_approvals: Vec<Approval>,
    },
    BuildHistory {
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
    Error(String),
}
