use chrono::{DateTime, Utc};
use serde::Deserialize;

// --- Pipeline Definitions ---

#[derive(Debug, Clone, Deserialize)]
pub struct DefinitionListResponse {
    pub value: Vec<PipelineDefinition>,
    #[allow(dead_code)]
    pub count: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineDefinition {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[allow(dead_code)]
    #[serde(rename = "queueStatus")]
    pub queue_status: Option<String>,
}

// --- Builds ---

#[derive(Debug, Clone, Deserialize)]
pub struct BuildListResponse {
    pub value: Vec<Build>,
    #[allow(dead_code)]
    pub count: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Build {
    pub id: u32,
    #[serde(rename = "buildNumber")]
    pub build_number: String,
    pub status: String,
    pub result: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "queueTime")]
    pub queue_time: Option<DateTime<Utc>>,
    #[serde(rename = "startTime")]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(rename = "finishTime")]
    pub finish_time: Option<DateTime<Utc>>,
    pub definition: BuildDefinitionRef,
    #[serde(rename = "sourceBranch")]
    pub source_branch: Option<String>,
    #[serde(rename = "requestedFor")]
    pub requested_for: Option<IdentityRef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildDefinitionRef {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdentityRef {
    #[serde(rename = "displayName")]
    pub display_name: String,
}

// --- Build Logs ---

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct BuildLogListResponse {
    pub value: Vec<BuildLogEntry>,
    pub count: u32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct BuildLogEntry {
    pub id: u32,
    #[serde(rename = "type")]
    pub log_type: Option<String>,
    #[serde(rename = "lineCount")]
    pub line_count: Option<u32>,
    #[serde(rename = "createdOn")]
    pub created_on: Option<DateTime<Utc>>,
}

// --- Build Timeline (stages/jobs/tasks) ---

#[derive(Debug, Clone, Deserialize)]
pub struct BuildTimeline {
    pub records: Vec<TimelineRecord>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TimelineRecord {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub state: Option<String>,
    pub result: Option<String>,
    pub order: Option<i32>,
    #[serde(rename = "log")]
    pub log: Option<LogReference>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogReference {
    pub id: u32,
}

impl Build {
    pub fn short_branch(&self) -> String {
        self.source_branch
            .as_deref()
            .unwrap_or("")
            .strip_prefix("refs/heads/")
            .or_else(|| {
                self.source_branch
                    .as_deref()
                    .unwrap_or("")
                    .strip_prefix("refs/pull/")
            })
            .unwrap_or(self.source_branch.as_deref().unwrap_or(""))
            .to_string()
    }

    pub fn requestor(&self) -> &str {
        self.requested_for
            .as_ref()
            .map(|r| r.display_name.as_str())
            .unwrap_or("Unknown")
    }
}
