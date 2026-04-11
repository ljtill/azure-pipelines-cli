//! Azure DevOps pipeline approval model types.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::builds::IdentityRef;

// --- Approvals ---

/// Represents a paginated list of approvals.
#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalListResponse {
    pub value: Vec<Approval>,
    #[allow(dead_code)]
    pub count: u32,
}

/// Represents a pipeline approval request.
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
    /// References the pipeline/build linking this approval to a specific run.
    pub pipeline: Option<ApprovalPipelineRef>,
}

/// Represents a pipeline reference within an approval response.
#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalPipelineRef {
    /// References the build/run that triggered this approval.
    pub owner: Option<ApprovalOwnerRef>,
}

/// Represents a build/run owner reference within an approval's pipeline field.
#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalOwnerRef {
    /// Contains the build/run ID.
    pub id: u32,
}

/// Represents a single approval step assigned to an approver.
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

impl Approval {
    /// Extracts the build/run ID this approval is associated with.
    pub fn build_id(&self) -> Option<u32> {
        self.pipeline.as_ref()?.owner.as_ref().map(|o| o.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            ],
            "pipeline": {
                "owner": { "id": 12345, "name": "20240615.1" }
            }
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
        assert_eq!(approval.build_id(), Some(12345));
    }

    #[test]
    fn deserialize_approval_without_pipeline() {
        let json = r#"{
            "id": "approval-no-pipeline",
            "status": "pending",
            "steps": []
        }"#;
        let approval: Approval = serde_json::from_str(json).unwrap();
        assert_eq!(approval.build_id(), None);
    }
}
