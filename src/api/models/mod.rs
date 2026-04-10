pub mod approvals;
pub mod builds;
pub mod definitions;
pub mod retention;

pub use approvals::*;
pub use builds::*;
pub use definitions::*;
pub use retention::*;

use std::fmt;

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

    // --- ListResponse ---

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
}
