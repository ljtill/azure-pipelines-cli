//! Azure DevOps retention lease model types.

use chrono::{DateTime, Utc};
use serde::Deserialize;

// --- Retention Leases ---

/// Represents a retention lease that prevents automated systems from deleting a pipeline run.
#[derive(Debug, Clone, Deserialize)]
pub struct RetentionLease {
    #[serde(rename = "leaseId")]
    pub lease_id: u32,
    #[serde(rename = "definitionId")]
    pub definition_id: u32,
    #[serde(rename = "runId")]
    pub run_id: u32,
    #[serde(rename = "ownerId")]
    pub owner_id: String,
    #[serde(rename = "createdOn")]
    pub created_on: Option<DateTime<Utc>>,
    #[serde(rename = "validUntil")]
    pub valid_until: Option<DateTime<Utc>>,
    #[serde(rename = "protectPipeline", default)]
    pub protect_pipeline: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::models::ListResponse;

    #[test]
    fn deserialize_retention_lease() {
        let json = r#"{
            "leaseId": 101,
            "definitionId": 5,
            "runId": 42,
            "ownerId": "System:Pipeline",
            "createdOn": "2024-06-15T10:00:00Z",
            "validUntil": "2025-06-15T10:00:00Z",
            "protectPipeline": true
        }"#;
        let lease: RetentionLease = serde_json::from_str(json).unwrap();
        assert_eq!(lease.lease_id, 101);
        assert_eq!(lease.definition_id, 5);
        assert_eq!(lease.run_id, 42);
        assert_eq!(lease.owner_id, "System:Pipeline");
        assert!(lease.created_on.is_some());
        assert!(lease.valid_until.is_some());
        assert!(lease.protect_pipeline);
    }

    #[test]
    fn deserialize_retention_lease_minimal() {
        let json = r#"{
            "leaseId": 200,
            "definitionId": 10,
            "runId": 99,
            "ownerId": "User:abc"
        }"#;
        let lease: RetentionLease = serde_json::from_str(json).unwrap();
        assert_eq!(lease.lease_id, 200);
        assert_eq!(lease.definition_id, 10);
        assert_eq!(lease.run_id, 99);
        assert_eq!(lease.owner_id, "User:abc");
        assert!(lease.created_on.is_none());
        assert!(lease.valid_until.is_none());
        assert!(!lease.protect_pipeline);
    }

    #[test]
    fn deserialize_retention_lease_list_response() {
        let json = r#"{
            "value": [
                {
                    "leaseId": 1,
                    "definitionId": 5,
                    "runId": 10,
                    "ownerId": "System:Pipeline",
                    "protectPipeline": false
                },
                {
                    "leaseId": 2,
                    "definitionId": 5,
                    "runId": 11,
                    "ownerId": "User:admin",
                    "protectPipeline": true
                }
            ],
            "count": 2
        }"#;
        let resp: ListResponse<RetentionLease> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.value.len(), 2);
        assert_eq!(resp.value[0].lease_id, 1);
        assert_eq!(resp.value[1].lease_id, 2);
        assert!(resp.value[1].protect_pipeline);
    }
}
