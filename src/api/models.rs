use std::fmt;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde::de::{self, Visitor};

// --- Enums for build status and result ---

/// Build status as returned by the Azure DevOps API.
/// Deserialized case-insensitively with an `Unknown` fallback for unrecognized values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStatus {
    None,
    InProgress,
    Completed,
    Cancelling,
    Postponed,
    NotStarted,
    Unknown,
}

impl BuildStatus {
    pub fn is_in_progress(self) -> bool {
        self == BuildStatus::InProgress
    }
}

impl<'de> Deserialize<'de> for BuildStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct StatusVisitor;
        impl<'de> Visitor<'de> for StatusVisitor {
            type Value = BuildStatus;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a build status string")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<BuildStatus, E> {
                Ok(match v.to_ascii_lowercase().as_str() {
                    "none" => BuildStatus::None,
                    "inprogress" => BuildStatus::InProgress,
                    "completed" => BuildStatus::Completed,
                    "cancelling" => BuildStatus::Cancelling,
                    "postponed" => BuildStatus::Postponed,
                    "notstarted" => BuildStatus::NotStarted,
                    _ => BuildStatus::Unknown,
                })
            }
        }
        deserializer.deserialize_str(StatusVisitor)
    }
}

impl fmt::Display for BuildStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildStatus::None => write!(f, "none"),
            BuildStatus::InProgress => write!(f, "inProgress"),
            BuildStatus::Completed => write!(f, "completed"),
            BuildStatus::Cancelling => write!(f, "cancelling"),
            BuildStatus::Postponed => write!(f, "postponed"),
            BuildStatus::NotStarted => write!(f, "notStarted"),
            BuildStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// Build or timeline record result.
/// Deserialized case-insensitively with an `Unknown` fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildResult {
    None,
    Succeeded,
    PartiallySucceeded,
    Failed,
    Canceled,
    Skipped,
    Unknown,
}

impl<'de> Deserialize<'de> for BuildResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct ResultVisitor;
        impl<'de> Visitor<'de> for ResultVisitor {
            type Value = BuildResult;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a build result string")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<BuildResult, E> {
                Ok(match v.to_ascii_lowercase().as_str() {
                    "none" => BuildResult::None,
                    "succeeded" => BuildResult::Succeeded,
                    "partiallysucceeded" => BuildResult::PartiallySucceeded,
                    "failed" => BuildResult::Failed,
                    "canceled" | "cancelled" => BuildResult::Canceled,
                    "skipped" => BuildResult::Skipped,
                    _ => BuildResult::Unknown,
                })
            }
        }
        deserializer.deserialize_str(ResultVisitor)
    }
}

/// Timeline record state (stage/job/task).
/// Deserialized case-insensitively with an `Unknown` fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    InProgress,
    Completed,
    Unknown,
}

impl TaskState {
    pub fn is_in_progress(self) -> bool {
        self == TaskState::InProgress
    }
}

impl<'de> Deserialize<'de> for TaskState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct StateVisitor;
        impl<'de> Visitor<'de> for StateVisitor {
            type Value = TaskState;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a timeline state string")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<TaskState, E> {
                Ok(match v.to_ascii_lowercase().as_str() {
                    "pending" => TaskState::Pending,
                    "inprogress" => TaskState::InProgress,
                    "completed" => TaskState::Completed,
                    _ => TaskState::Unknown,
                })
            }
        }
        deserializer.deserialize_str(StateVisitor)
    }
}

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
    pub status: BuildStatus,
    pub result: Option<BuildResult>,
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
    pub identifier: Option<String>,
    #[serde(rename = "type")]
    pub record_type: String,
    pub state: Option<TaskState>,
    pub result: Option<BuildResult>,
    pub order: Option<i32>,
    #[serde(rename = "log")]
    pub log: Option<LogReference>,
}

// --- Pipeline Run (queue) ---

#[derive(Debug, serde::Serialize)]
pub struct RunPipelineRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<RunPipelineResources>,
}

#[derive(Debug, serde::Serialize)]
pub struct RunPipelineResources {
    pub repositories: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineRun {
    pub id: u32,
    pub name: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- BuildStatus deserialization ---

    #[test]
    fn deserialize_build_status_camel_case() {
        let status: BuildStatus = serde_json::from_str(r#""inProgress""#).unwrap();
        assert_eq!(status, BuildStatus::InProgress);
    }

    #[test]
    fn deserialize_build_status_pascal_case() {
        let status: BuildStatus = serde_json::from_str(r#""InProgress""#).unwrap();
        assert_eq!(status, BuildStatus::InProgress);
    }

    #[test]
    fn deserialize_build_status_completed() {
        let status: BuildStatus = serde_json::from_str(r#""completed""#).unwrap();
        assert_eq!(status, BuildStatus::Completed);
    }

    #[test]
    fn deserialize_build_status_unknown_value() {
        let status: BuildStatus = serde_json::from_str(r#""somethingNew""#).unwrap();
        assert_eq!(status, BuildStatus::Unknown);
    }

    // --- BuildResult deserialization ---

    #[test]
    fn deserialize_build_result_succeeded() {
        let result: BuildResult = serde_json::from_str(r#""succeeded""#).unwrap();
        assert_eq!(result, BuildResult::Succeeded);
    }

    #[test]
    fn deserialize_build_result_failed() {
        let result: BuildResult = serde_json::from_str(r#""failed""#).unwrap();
        assert_eq!(result, BuildResult::Failed);
    }

    #[test]
    fn deserialize_build_result_canceled_us() {
        let result: BuildResult = serde_json::from_str(r#""canceled""#).unwrap();
        assert_eq!(result, BuildResult::Canceled);
    }

    #[test]
    fn deserialize_build_result_cancelled_uk() {
        let result: BuildResult = serde_json::from_str(r#""cancelled""#).unwrap();
        assert_eq!(result, BuildResult::Canceled);
    }

    #[test]
    fn deserialize_build_result_partially_succeeded() {
        let result: BuildResult = serde_json::from_str(r#""partiallySucceeded""#).unwrap();
        assert_eq!(result, BuildResult::PartiallySucceeded);
    }

    // --- TaskState deserialization ---

    #[test]
    fn deserialize_task_state_in_progress() {
        let state: TaskState = serde_json::from_str(r#""inProgress""#).unwrap();
        assert_eq!(state, TaskState::InProgress);
    }

    #[test]
    fn deserialize_task_state_pending() {
        let state: TaskState = serde_json::from_str(r#""pending""#).unwrap();
        assert_eq!(state, TaskState::Pending);
    }

    #[test]
    fn deserialize_task_state_completed() {
        let state: TaskState = serde_json::from_str(r#""completed""#).unwrap();
        assert_eq!(state, TaskState::Completed);
    }

    // --- Build helper methods ---

    #[test]
    fn short_branch_strips_refs_heads() {
        let build = Build {
            id: 1,
            build_number: "1".to_string(),
            status: BuildStatus::Completed,
            result: Some(BuildResult::Succeeded),
            queue_time: None,
            start_time: None,
            finish_time: None,
            definition: BuildDefinitionRef {
                id: 1,
                name: "test".to_string(),
            },
            source_branch: Some("refs/heads/main".to_string()),
            requested_for: None,
        };
        assert_eq!(build.short_branch(), "main");
    }

    #[test]
    fn short_branch_strips_refs_pull() {
        let build = Build {
            id: 1,
            build_number: "1".to_string(),
            status: BuildStatus::Completed,
            result: None,
            queue_time: None,
            start_time: None,
            finish_time: None,
            definition: BuildDefinitionRef {
                id: 1,
                name: "test".to_string(),
            },
            source_branch: Some("refs/pull/42/merge".to_string()),
            requested_for: None,
        };
        assert_eq!(build.short_branch(), "42/merge");
    }

    #[test]
    fn requestor_unknown_when_none() {
        let build = Build {
            id: 1,
            build_number: "1".to_string(),
            status: BuildStatus::Completed,
            result: None,
            queue_time: None,
            start_time: None,
            finish_time: None,
            definition: BuildDefinitionRef {
                id: 1,
                name: "test".to_string(),
            },
            source_branch: None,
            requested_for: None,
        };
        assert_eq!(build.requestor(), "Unknown");
    }
}
