//! Azure DevOps build and timeline model types.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{BuildResult, BuildStatus};

// --- Builds ---

/// Represents a paginated list of builds.
#[derive(Debug, Clone, Deserialize)]
pub struct BuildListResponse {
    pub value: Vec<Build>,
    #[allow(dead_code)]
    pub count: u32,
}

/// Represents a single Azure DevOps build.
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
    pub reason: Option<String>,
    #[serde(rename = "triggerInfo")]
    pub trigger_info: Option<HashMap<String, String>>,
}

/// Represents a minimal reference to a build definition.
#[derive(Debug, Clone, Deserialize)]
pub struct BuildDefinitionRef {
    pub id: u32,
    pub name: String,
}

/// Represents an identity reference with a display name.
#[derive(Debug, Clone, Deserialize)]
pub struct IdentityRef {
    #[serde(rename = "displayName")]
    pub display_name: String,
}

// --- Build Timeline (stages/jobs/tasks) ---

/// Represents a build timeline containing stage, job, and task records.
#[derive(Debug, Clone, Deserialize)]
pub struct BuildTimeline {
    pub records: Vec<TimelineRecord>,
}

/// Represents a single record in a build timeline.
#[derive(Debug, Clone, Deserialize)]
pub struct TimelineRecord {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub name: String,
    pub identifier: Option<String>,
    #[serde(rename = "type")]
    pub record_type: String,
    pub state: Option<super::TaskState>,
    pub result: Option<BuildResult>,
    pub order: Option<i32>,
    #[serde(rename = "log")]
    pub log: Option<LogReference>,
}

// --- Pipeline Run (queue) ---

/// Represents a queued pipeline run.
#[derive(Debug, Clone, Deserialize)]
pub struct PipelineRun {
    pub id: u32,
    #[allow(dead_code)]
    pub name: String,
}

/// Represents a reference to a log resource.
#[derive(Debug, Clone, Deserialize)]
pub struct LogReference {
    pub id: u32,
}

impl Build {
    /// Returns the branch name with `refs/heads/` or `refs/pull/` prefix stripped.
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

    /// Returns the display name of the user who requested the build.
    pub fn requestor(&self) -> &str {
        self.requested_for
            .as_ref()
            .map(|r| r.display_name.as_str())
            .unwrap_or("Unknown")
    }

    /// Returns `true` if this build was triggered by a pull request.
    pub fn is_pr_build(&self) -> bool {
        self.reason
            .as_deref()
            .is_some_and(|r| r.eq_ignore_ascii_case("pullRequest"))
    }

    /// Returns the pull request title from trigger info, if available.
    pub fn pr_title(&self) -> Option<&str> {
        self.trigger_info
            .as_ref()
            .and_then(|ti| ti.get("pr.title").map(|s| s.as_str()))
    }

    /// Returns the pull request number from trigger info, if available.
    pub fn pr_number(&self) -> Option<&str> {
        self.trigger_info
            .as_ref()
            .and_then(|ti| ti.get("pr.number").map(|s| s.as_str()))
    }

    /// Returns the user-facing branch or PR description for list views.
    ///
    /// For PR builds with trigger info: `PR #42 · Fix login timeout`
    /// For other builds: the short branch name (e.g. `main`, `feat/widget`).
    pub fn branch_display(&self) -> String {
        if self.is_pr_build()
            && let Some(number) = self.pr_number()
        {
            if let Some(title) = self.pr_title() {
                return format!("PR #{} · {}", number, title);
            }
            return format!("PR #{}", number);
        }
        self.short_branch()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            reason: None,
            trigger_info: None,
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
            reason: None,
            trigger_info: None,
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
            reason: None,
            trigger_info: None,
        };
        assert_eq!(build.requestor(), "Unknown");
    }

    // --- PR helper methods ---

    #[test]
    fn is_pr_build_true() {
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
            reason: Some("pullRequest".to_string()),
            trigger_info: None,
        };
        assert!(build.is_pr_build());
    }

    #[test]
    fn is_pr_build_case_insensitive() {
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
            reason: Some("PullRequest".to_string()),
            trigger_info: None,
        };
        assert!(build.is_pr_build());
    }

    #[test]
    fn is_pr_build_false_for_ci() {
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
            reason: Some("individualCI".to_string()),
            trigger_info: None,
        };
        assert!(!build.is_pr_build());
    }

    #[test]
    fn branch_display_pr_with_title() {
        let mut ti = HashMap::new();
        ti.insert("pr.number".to_string(), "42".to_string());
        ti.insert("pr.title".to_string(), "Fix login timeout".to_string());
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
            reason: Some("pullRequest".to_string()),
            trigger_info: Some(ti),
        };
        assert_eq!(build.branch_display(), "PR #42 · Fix login timeout");
    }

    #[test]
    fn branch_display_pr_without_title() {
        let mut ti = HashMap::new();
        ti.insert("pr.number".to_string(), "99".to_string());
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
            source_branch: Some("refs/pull/99/merge".to_string()),
            requested_for: None,
            reason: Some("pullRequest".to_string()),
            trigger_info: Some(ti),
        };
        assert_eq!(build.branch_display(), "PR #99");
    }

    #[test]
    fn branch_display_pr_no_trigger_info_falls_back() {
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
            reason: Some("pullRequest".to_string()),
            trigger_info: None,
        };
        assert_eq!(build.branch_display(), "42/merge");
    }

    #[test]
    fn branch_display_ci_build() {
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
            source_branch: Some("refs/heads/main".to_string()),
            requested_for: None,
            reason: Some("individualCI".to_string()),
            trigger_info: None,
        };
        assert_eq!(build.branch_display(), "main");
    }

    // --- Full object deserialization ---

    #[test]
    fn deserialize_full_build() {
        let json = r#"{
            "id": 42,
            "buildNumber": "20240101.1",
            "status": "completed",
            "result": "succeeded",
            "reason": "individualCI",
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
        assert_eq!(build.reason.as_deref(), Some("individualCI"));
        assert!(build.trigger_info.is_none());
        assert!(build.queue_time.is_some());
        assert!(build.start_time.is_some());
        assert!(build.finish_time.is_some());
        assert_eq!(build.definition.id, 1);
        assert_eq!(build.definition.name, "CI");
        assert_eq!(build.source_branch.as_deref(), Some("refs/heads/main"));
        assert_eq!(build.requestor(), "Jane Doe");
        assert!(!build.is_pr_build());
        assert_eq!(build.branch_display(), "main");
    }

    #[test]
    fn deserialize_pr_build_with_trigger_info() {
        let json = r#"{
            "id": 55,
            "buildNumber": "20240301.5",
            "status": "completed",
            "result": "failed",
            "reason": "pullRequest",
            "definition": {"id": 2, "name": "PR Validation"},
            "sourceBranch": "refs/pull/42/merge",
            "triggerInfo": {
                "pr.number": "42",
                "pr.title": "Fix login timeout",
                "pr.sourceBranch": "refs/heads/fix/login"
            }
        }"#;
        let build: Build = serde_json::from_str(json).unwrap();
        assert_eq!(build.id, 55);
        assert!(build.is_pr_build());
        assert_eq!(build.pr_number(), Some("42"));
        assert_eq!(build.pr_title(), Some("Fix login timeout"));
        assert_eq!(build.branch_display(), "PR #42 · Fix login timeout");
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
        assert_eq!(rec.state, Some(super::super::TaskState::Completed));
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
        assert_eq!(rec.state, Some(super::super::TaskState::Pending));
        assert!(rec.result.is_none());
        assert!(rec.log.is_none());
    }
}
