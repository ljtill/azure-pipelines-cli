//! Azure DevOps pipeline definition model types.

use serde::Deserialize;

use super::builds::Build;

// --- Pipeline Definitions ---

/// Represents an Azure DevOps pipeline definition.
#[derive(Debug, Clone, Deserialize)]
pub struct PipelineDefinition {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[allow(dead_code)]
    #[serde(rename = "queueStatus")]
    pub queue_status: Option<String>,
    /// Contains the latest build, populated when `includeLatestBuilds=true`.
    #[serde(rename = "latestBuild")]
    pub latest_build: Option<Box<Build>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::models::*;

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
}
