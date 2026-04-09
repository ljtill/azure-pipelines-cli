use crate::api::models::{Build, BuildTimeline, PipelineDefinition};

/// Messages sent from background tasks to the main event loop.
pub enum AppMessage {
    DataRefresh {
        definitions: Vec<PipelineDefinition>,
        recent_builds: Vec<Build>,
        active_builds: Vec<Build>,
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
    StageRetried,
    PipelineQueued {
        build: Build,
        #[allow(dead_code)]
        definition_id: u32,
    },
    Error(String),
}
