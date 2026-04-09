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

// --- Generic paginated list response ---

#[derive(Debug, Clone, Deserialize)]
pub struct ListResponse<T> {
    pub value: Vec<T>,
    #[allow(dead_code)]
    pub count: Option<u32>,
}

// --- Pipeline Definitions ---

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineDefinition {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[allow(dead_code)]
    #[serde(rename = "queueStatus")]
    pub queue_status: Option<String>,
    /// Latest build for this definition, populated when `includeLatestBuilds=true`.
    #[serde(rename = "latestBuild")]
    pub latest_build: Option<Box<Build>>,
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

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineRun {
    pub id: u32,
    #[allow(dead_code)]
    pub name: String,
}

// --- Approvals ---

#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalListResponse {
    pub value: Vec<Approval>,
    #[allow(dead_code)]
    pub count: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Approval {
    pub id: String,
    pub status: String,
    #[allow(dead_code)]
    pub instructions: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "createdOn")]
    pub created_on: Option<DateTime<Utc>>,
    #[allow(dead_code)]
    #[serde(default)]
    pub steps: Vec<ApprovalStep>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalStep {
    #[allow(dead_code)]
    #[serde(rename = "assignedApprover")]
    pub assigned_approver: Option<IdentityRef>,
    #[allow(dead_code)]
    pub status: Option<String>,
    #[allow(dead_code)]
    pub order: Option<i32>,
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

    // --- Full object deserialization ---

    #[test]
    fn deserialize_full_build() {
        let json = r#"{
            "id": 42,
            "buildNumber": "20240101.1",
            "status": "completed",
            "result": "succeeded",
            "queueTime": "2024-01-01T10:00:00Z",
            "startTime": "2024-01-01T10:00:05Z",
            "finishTime": "2024-01-01T10:05:00Z",
            "definition": {"id": 1, "name": "CI"},
            "sourceBranch": "refs/heads/main",
            "requestedFor": {"displayName": "Jane Doe"}
        }"#;
        let build: Build = serde_json::from_str(json).unwrap();
        assert_eq!(build.id, 42);
        assert_eq!(build.build_number, "20240101.1");
        assert_eq!(build.status, BuildStatus::Completed);
        assert_eq!(build.result, Some(BuildResult::Succeeded));
        assert!(build.queue_time.is_some());
        assert!(build.start_time.is_some());
        assert!(build.finish_time.is_some());
        assert_eq!(build.definition.id, 1);
        assert_eq!(build.definition.name, "CI");
        assert_eq!(build.source_branch.as_deref(), Some("refs/heads/main"));
        assert_eq!(build.requestor(), "Jane Doe");
    }

    #[test]
    fn deserialize_build_no_optional_fields() {
        let json = r#"{
            "id": 99,
            "buildNumber": "20240201.3",
            "status": "inProgress",
            "definition": {"id": 5, "name": "Nightly"}
        }"#;
        let build: Build = serde_json::from_str(json).unwrap();
        assert_eq!(build.id, 99);
        assert_eq!(build.status, BuildStatus::InProgress);
        assert!(build.result.is_none());
        assert!(build.finish_time.is_none());
        assert!(build.requested_for.is_none());
        assert_eq!(build.requestor(), "Unknown");
    }

    #[test]
    fn deserialize_definition() {
        let json = r#"{
            "id": 10,
            "name": "Release Pipeline",
            "path": "\\Production",
            "queueStatus": "enabled"
        }"#;
        let def: PipelineDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(def.id, 10);
        assert_eq!(def.name, "Release Pipeline");
        assert_eq!(def.path, "\\Production");
        assert_eq!(def.queue_status.as_deref(), Some("enabled"));
        assert!(def.latest_build.is_none());
    }

    #[test]
    fn deserialize_definition_with_latest_build() {
        let json = r#"{
            "id": 10,
            "name": "Release Pipeline",
            "path": "\\Production",
            "queueStatus": "enabled",
            "latestBuild": {
                "id": 42,
                "buildNumber": "20240101.1",
                "status": "completed",
                "result": "succeeded",
                "definition": {"id": 10, "name": "Release Pipeline"},
                "sourceBranch": "refs/heads/main"
            }
        }"#;
        let def: PipelineDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(def.id, 10);
        let build = def.latest_build.expect("latestBuild should be present");
        assert_eq!(build.id, 42);
        assert_eq!(build.status, BuildStatus::Completed);
        assert_eq!(build.result, Some(BuildResult::Succeeded));
    }

    #[test]
    fn deserialize_list_response_with_count() {
        let json = r#"{
            "value": [
                {"id": 1, "name": "CI", "path": "\\", "queueStatus": "enabled"},
                {"id": 2, "name": "CD", "path": "\\Deploy"}
            ],
            "count": 2
        }"#;
        let resp: ListResponse<PipelineDefinition> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.count, Some(2));
        assert_eq!(resp.value.len(), 2);
        assert_eq!(resp.value[0].name, "CI");
        assert_eq!(resp.value[1].path, "\\Deploy");
    }

    #[test]
    fn deserialize_list_response_without_count() {
        let json = r#"{
            "value": [
                {"id": 3, "name": "Nightly", "path": "\\"}
            ]
        }"#;
        let resp: ListResponse<PipelineDefinition> = serde_json::from_str(json).unwrap();
        assert!(resp.count.is_none());
        assert_eq!(resp.value.len(), 1);
        assert_eq!(resp.value[0].id, 3);
    }

    #[test]
    fn deserialize_list_response_empty() {
        let json = r#"{"value": [], "count": 0}"#;
        let resp: ListResponse<PipelineDefinition> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.count, Some(0));
        assert!(resp.value.is_empty());
    }

    #[test]
    fn deserialize_definition_list_response() {
        let json = r#"{
            "value": [
                {"id": 1, "name": "CI", "path": "\\", "queueStatus": "enabled"},
                {"id": 2, "name": "CD", "path": "\\Deploy"}
            ],
            "count": 2
        }"#;
        let resp: ListResponse<PipelineDefinition> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.count, Some(2));
        assert_eq!(resp.value.len(), 2);
        assert_eq!(resp.value[0].name, "CI");
        assert_eq!(resp.value[1].path, "\\Deploy");
    }

    #[test]
    fn deserialize_build_timeline() {
        let json = r#"{
            "records": [
                {
                    "id": "stage-1",
                    "parentId": null,
                    "name": "Build Stage",
                    "identifier": "build_stage",
                    "type": "Stage",
                    "state": "completed",
                    "result": "succeeded",
                    "order": 1,
                    "log": null
                },
                {
                    "id": "task-1",
                    "parentId": "stage-1",
                    "name": "Run Tests",
                    "identifier": "run_tests",
                    "type": "Task",
                    "state": "completed",
                    "result": "succeeded",
                    "order": 1,
                    "log": {"id": 5}
                }
            ]
        }"#;
        let tl: BuildTimeline = serde_json::from_str(json).unwrap();
        assert_eq!(tl.records.len(), 2);
        assert_eq!(tl.records[0].record_type, "Stage");
        assert!(tl.records[0].parent_id.is_none());
        assert_eq!(tl.records[1].parent_id.as_deref(), Some("stage-1"));
        assert!(tl.records[1].log.is_some());
        assert_eq!(tl.records[1].log.as_ref().unwrap().id, 5);
    }

    #[test]
    fn deserialize_approval() {
        let json = r#"{
            "id": "approval-abc",
            "status": "pending",
            "instructions": "Please approve the release",
            "createdOn": "2024-06-15T12:00:00Z",
            "steps": [
                {
                    "assignedApprover": {"displayName": "Alice"},
                    "status": "pending",
                    "order": 1
                }
            ]
        }"#;
        let approval: Approval = serde_json::from_str(json).unwrap();
        assert_eq!(approval.id, "approval-abc");
        assert_eq!(approval.status, "pending");
        assert_eq!(
            approval.instructions.as_deref(),
            Some("Please approve the release")
        );
        assert!(approval.created_on.is_some());
        assert_eq!(approval.steps.len(), 1);
        assert_eq!(
            approval.steps[0]
                .assigned_approver
                .as_ref()
                .unwrap()
                .display_name,
            "Alice"
        );
        assert_eq!(approval.steps[0].order, Some(1));
    }

    #[test]
    fn deserialize_timeline_record_with_log() {
        let json = r#"{
            "id": "rec-1",
            "parentId": "parent-1",
            "name": "Compile",
            "identifier": "compile_step",
            "type": "Task",
            "state": "completed",
            "result": "succeeded",
            "order": 2,
            "log": {"id": 42}
        }"#;
        let rec: TimelineRecord = serde_json::from_str(json).unwrap();
        assert_eq!(rec.id, "rec-1");
        assert_eq!(rec.parent_id.as_deref(), Some("parent-1"));
        assert_eq!(rec.name, "Compile");
        assert_eq!(rec.record_type, "Task");
        assert_eq!(rec.state, Some(TaskState::Completed));
        assert_eq!(rec.result, Some(BuildResult::Succeeded));
        assert!(rec.log.is_some());
        assert_eq!(rec.log.unwrap().id, 42);
    }

    #[test]
    fn deserialize_timeline_record_without_log() {
        let json = r#"{
            "id": "rec-2",
            "name": "Deploy Stage",
            "type": "Stage",
            "state": "pending",
            "order": 1
        }"#;
        let rec: TimelineRecord = serde_json::from_str(json).unwrap();
        assert_eq!(rec.id, "rec-2");
        assert!(rec.parent_id.is_none());
        assert_eq!(rec.record_type, "Stage");
        assert_eq!(rec.state, Some(TaskState::Pending));
        assert!(rec.result.is_none());
        assert!(rec.log.is_none());
    }
}
