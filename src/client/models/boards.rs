//! Azure Boards team, backlog, WIQL, and work item model types.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::builds::IdentityRef;

/// Represents a project team.
#[derive(Debug, Clone, Deserialize)]
pub struct ProjectTeam {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "projectId")]
    pub project_id: Option<String>,
    #[serde(rename = "projectName")]
    pub project_name: Option<String>,
    pub url: Option<String>,
}

impl ProjectTeam {
    /// Returns `true` when this is the default project team.
    pub fn is_default_project_team(&self) -> bool {
        self.description
            .as_deref()
            .is_some_and(|desc| desc.eq_ignore_ascii_case("The default project team."))
    }
}

/// Represents a backlog level definition for a team.
#[derive(Debug, Clone, Deserialize)]
pub struct BacklogLevelConfiguration {
    pub id: String,
    pub name: String,
    pub rank: u32,
    #[serde(rename = "workItemCountLimit")]
    pub work_item_count_limit: Option<u32>,
    #[serde(rename = "workItemTypes", default)]
    pub work_item_types: Vec<WorkItemTypeReference>,
    #[serde(rename = "defaultWorkItemType")]
    pub default_work_item_type: Option<WorkItemTypeReference>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(rename = "isHidden", default)]
    pub is_hidden: bool,
    #[serde(rename = "type")]
    pub backlog_type: Option<String>,
}

impl BacklogLevelConfiguration {
    /// Returns `true` when this backlog level should be shown.
    pub fn is_visible(&self) -> bool {
        !self.is_hidden
    }

    /// Returns the names of all work item types on this backlog level.
    pub fn work_item_type_names(&self) -> Vec<String> {
        self.work_item_types
            .iter()
            .map(|wit| wit.name.clone())
            .collect()
    }
}

/// Represents a work item type reference.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemTypeReference {
    pub name: String,
    pub url: Option<String>,
}

/// Represents work items within a backlog level.
#[derive(Debug, Clone, Deserialize)]
pub struct BacklogLevelWorkItems {
    #[serde(rename = "workItems", default)]
    pub work_items: Vec<WorkItemLink>,
}

/// Represents a work item type category.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemTypeCategory {
    pub name: String,
    #[serde(rename = "referenceName")]
    pub reference_name: String,
    #[serde(rename = "defaultWorkItemType")]
    pub default_work_item_type: Option<WorkItemTypeReference>,
    #[serde(rename = "workItemTypes", default)]
    pub work_item_types: Vec<WorkItemTypeReference>,
    pub url: Option<String>,
}

/// Represents a WIQL query body.
#[derive(Debug, Clone, Serialize)]
pub struct WiqlQuery {
    pub query: String,
}

/// Represents the result of a WIQL query.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemQueryResult {
    #[serde(rename = "queryType")]
    pub query_type: Option<String>,
    #[serde(rename = "queryResultType")]
    pub query_result_type: Option<String>,
    #[serde(rename = "workItemRelations", default)]
    pub work_item_relations: Vec<WorkItemLink>,
    #[serde(rename = "workItems", default)]
    pub work_items: Vec<WorkItemReference>,
}

/// Represents a work item reference.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemReference {
    pub id: u32,
    pub url: Option<String>,
}

/// Represents a link between work items.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemLink {
    pub rel: Option<String>,
    pub source: Option<WorkItemReference>,
    pub target: Option<WorkItemReference>,
}

/// Represents a batch request for work items.
#[derive(Debug, Clone, Serialize)]
pub struct WorkItemBatchGetRequest {
    pub ids: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    #[serde(rename = "$expand", skip_serializing_if = "Option::is_none")]
    pub expand: Option<WorkItemExpand>,
    #[serde(rename = "errorPolicy", skip_serializing_if = "Option::is_none")]
    pub error_policy: Option<WorkItemErrorPolicy>,
}

/// Represents the expand mode for a work item batch request.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum WorkItemExpand {
    None,
    Relations,
    Fields,
    Links,
    All,
}

/// Represents the error policy for a work item batch request.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum WorkItemErrorPolicy {
    Fail,
    Omit,
}

/// Represents a work item returned by the REST API.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkItem {
    pub id: u32,
    pub rev: Option<u32>,
    #[serde(default)]
    pub fields: WorkItemFields,
    #[serde(default)]
    pub relations: Vec<WorkItemRelation>,
    pub url: Option<String>,
}

/// Represents selected work item fields used by the Boards experience.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkItemFields {
    #[serde(rename = "System.Title", default)]
    pub title: String,
    #[serde(rename = "System.WorkItemType", default)]
    pub work_item_type: String,
    #[serde(rename = "System.State")]
    pub state: Option<String>,
    #[serde(rename = "System.AssignedTo")]
    pub assigned_to: Option<AssignedToField>,
    #[serde(rename = "System.IterationPath")]
    pub iteration_path: Option<String>,
    #[serde(rename = "System.AreaPath")]
    pub area_path: Option<String>,
    #[serde(rename = "System.Parent")]
    pub parent: Option<u32>,
    #[serde(rename = "System.BoardColumn")]
    pub board_column: Option<String>,
    #[serde(rename = "Microsoft.VSTS.Common.StackRank")]
    pub stack_rank: Option<f64>,

    // --- Detail fields (populated by the detail view) ---
    #[serde(rename = "System.Description")]
    pub description: Option<String>,
    #[serde(rename = "Microsoft.VSTS.Common.AcceptanceCriteria")]
    pub acceptance_criteria: Option<String>,
    #[serde(rename = "Microsoft.VSTS.TCM.ReproSteps")]
    pub repro_steps: Option<String>,
    #[serde(rename = "Microsoft.VSTS.Common.Priority")]
    pub priority: Option<i32>,
    #[serde(rename = "Microsoft.VSTS.Common.Severity")]
    pub severity: Option<String>,
    #[serde(rename = "Microsoft.VSTS.Common.ValueArea")]
    pub value_area: Option<String>,
    #[serde(rename = "Microsoft.VSTS.Scheduling.StoryPoints")]
    pub story_points: Option<f64>,
    #[serde(rename = "Microsoft.VSTS.Scheduling.Effort")]
    pub effort: Option<f64>,
    #[serde(rename = "System.Tags")]
    pub tags: Option<String>,
    #[serde(rename = "System.Reason")]
    pub reason: Option<String>,
    #[serde(rename = "System.CreatedBy")]
    pub created_by: Option<AssignedToField>,
    #[serde(rename = "System.CreatedDate")]
    pub created_date: Option<String>,
    #[serde(rename = "System.ChangedBy")]
    pub changed_by: Option<AssignedToField>,
    #[serde(rename = "System.ChangedDate")]
    pub changed_date: Option<String>,
}

/// Represents the assigned-to field, which may be an identity object or a string.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AssignedToField {
    Identity(IdentityRef),
    DisplayName(String),
}

impl AssignedToField {
    /// Returns the display name for the assigned-to field.
    pub fn display_name(&self) -> &str {
        match self {
            AssignedToField::Identity(identity) => identity.display_name.as_str(),
            AssignedToField::DisplayName(value) => value.as_str(),
        }
    }
}

/// Represents a work item relation.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemRelation {
    pub rel: Option<String>,
    pub url: String,
    #[serde(default)]
    pub attributes: HashMap<String, serde_json::Value>,
}

/// Represents a comment on a work item returned by the `comments` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemComment {
    pub id: u32,
    #[serde(default)]
    pub text: String,
    #[serde(rename = "createdBy")]
    pub created_by: Option<IdentityRef>,
    #[serde(rename = "createdDate")]
    pub created_date: Option<String>,
    #[serde(rename = "modifiedBy")]
    pub modified_by: Option<IdentityRef>,
    #[serde(rename = "modifiedDate")]
    pub modified_date: Option<String>,
}

/// Represents the envelope returned by the work item comments API.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkItemCommentList {
    #[serde(default)]
    pub comments: Vec<WorkItemComment>,
    #[serde(rename = "totalCount", default)]
    pub total_count: u32,
    #[serde(rename = "continuationToken")]
    pub continuation_token: Option<String>,
}

impl WorkItem {
    /// Returns the work item title.
    pub fn title(&self) -> &str {
        self.fields.title.as_str()
    }

    /// Returns the work item type label.
    pub fn work_item_type(&self) -> &str {
        self.fields.work_item_type.as_str()
    }

    /// Returns the work item state label.
    pub fn state_label(&self) -> &str {
        self.fields.state.as_deref().unwrap_or("")
    }

    /// Returns the assigned-to display name, if present.
    pub fn assigned_to_display(&self) -> Option<&str> {
        self.fields
            .assigned_to
            .as_ref()
            .map(AssignedToField::display_name)
    }

    /// Returns the parent work item ID, if present.
    pub fn parent_id(&self) -> Option<u32> {
        self.fields.parent.or_else(|| {
            self.relations.iter().find_map(|relation| {
                if relation.rel.as_deref().is_some_and(|rel| {
                    rel.eq_ignore_ascii_case("System.LinkTypes.Hierarchy-Reverse")
                }) {
                    relation.url.rsplit('/').next()?.parse().ok()
                } else {
                    None
                }
            })
        })
    }

    /// Returns the child work item IDs inferred from hierarchy-forward relations.
    pub fn child_ids(&self) -> Vec<u32> {
        let mut seen = HashSet::new();
        let mut ids = Vec::new();

        for relation in &self.relations {
            if relation
                .rel
                .as_deref()
                .is_some_and(|rel| rel.eq_ignore_ascii_case("System.LinkTypes.Hierarchy-Forward"))
                && let Some(id) = relation
                    .url
                    .rsplit('/')
                    .next()
                    .and_then(|value| value.parse().ok())
                && seen.insert(id)
            {
                ids.push(id);
            }
        }

        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_project_team_detection_matches_description() {
        let team = ProjectTeam {
            id: "1".to_string(),
            name: "Proj Team".to_string(),
            description: Some("The default project team.".to_string()),
            project_id: None,
            project_name: None,
            url: None,
        };

        assert!(team.is_default_project_team());
    }

    #[test]
    fn backlog_level_configuration_deserializes_visibility_and_work_item_types() {
        let backlog: BacklogLevelConfiguration = serde_json::from_str(
            r#"{
                "id":"Microsoft.EpicCategory",
                "name":"Epics",
                "rank":10,
                "workItemTypes":[{"name":"Epic","url":"https://example.invalid/epic"}],
                "defaultWorkItemType":{"name":"Epic","url":"https://example.invalid/epic"},
                "isHidden":true,
                "type":"portfolio"
            }"#,
        )
        .unwrap();

        assert!(!backlog.is_visible());
        assert_eq!(backlog.work_item_type_names(), vec!["Epic"]);
        assert_eq!(backlog.default_work_item_type.unwrap().name, "Epic");
    }

    #[test]
    fn assigned_to_field_deserializes_string() {
        let fields: WorkItemFields = serde_json::from_str(
            r#"{"System.Title":"Test","System.WorkItemType":"Task","System.AssignedTo":"Ada Lovelace <ada@example.com>"}"#,
        )
        .unwrap();

        assert_eq!(
            fields.assigned_to.as_ref().unwrap().display_name(),
            "Ada Lovelace <ada@example.com>"
        );
    }

    #[test]
    fn assigned_to_field_deserializes_identity() {
        let fields: WorkItemFields = serde_json::from_str(
            r#"{
                "System.Title":"Test",
                "System.WorkItemType":"Task",
                "System.AssignedTo":{
                    "id":"1",
                    "uniqueName":"ada@example.com",
                    "descriptor":"aad.1",
                    "displayName":"Ada Lovelace"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            fields.assigned_to.as_ref().unwrap().display_name(),
            "Ada Lovelace"
        );
    }

    #[test]
    fn parent_and_child_ids_are_derived_from_relations() {
        let work_item: WorkItem = serde_json::from_str(
            r#"{
                "id": 10,
                "fields": {
                    "System.Title": "Parent",
                    "System.WorkItemType": "Epic"
                },
                "relations": [
                    {
                        "rel": "System.LinkTypes.Hierarchy-Forward",
                        "url": "https://dev.azure.com/org/_apis/wit/workItems/11",
                        "attributes": {}
                    },
                    {
                        "rel": "System.LinkTypes.Hierarchy-Reverse",
                        "url": "https://dev.azure.com/org/_apis/wit/workItems/9",
                        "attributes": {}
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(work_item.parent_id(), Some(9));
        assert_eq!(work_item.child_ids(), vec![11]);
    }

    #[test]
    fn parent_id_prefers_field_and_child_ids_deduplicate_relations() {
        let work_item: WorkItem = serde_json::from_str(
            r#"{
                "id": 10,
                "fields": {
                    "System.Title": "Parent",
                    "System.WorkItemType": "Epic",
                    "System.Parent": 7
                },
                "relations": [
                    {
                        "rel": "System.LinkTypes.Hierarchy-Forward",
                        "url": "https://dev.azure.com/org/_apis/wit/workItems/11",
                        "attributes": {}
                    },
                    {
                        "rel": "System.LinkTypes.Hierarchy-Forward",
                        "url": "https://dev.azure.com/org/_apis/wit/workItems/11",
                        "attributes": {}
                    },
                    {
                        "rel": "System.LinkTypes.Hierarchy-Reverse",
                        "url": "https://dev.azure.com/org/_apis/wit/workItems/9",
                        "attributes": {}
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(work_item.parent_id(), Some(7));
        assert_eq!(work_item.child_ids(), vec![11]);
    }

    #[test]
    fn work_item_batch_request_serializes_pascal_case_enums() {
        let request = WorkItemBatchGetRequest {
            ids: vec![1, 2],
            fields: vec!["System.Title".to_string(), "System.State".to_string()],
            expand: Some(WorkItemExpand::Relations),
            error_policy: Some(WorkItemErrorPolicy::Omit),
        };

        let payload = serde_json::to_value(request).unwrap();

        assert_eq!(
            payload,
            serde_json::json!({
                "ids": [1, 2],
                "fields": ["System.Title", "System.State"],
                "$expand": "Relations",
                "errorPolicy": "Omit",
            })
        );
    }

    #[test]
    fn work_item_batch_request_skips_expand_when_none() {
        let request = WorkItemBatchGetRequest {
            ids: vec![1, 2],
            fields: vec!["System.Title".to_string(), "System.State".to_string()],
            expand: None,
            error_policy: Some(WorkItemErrorPolicy::Omit),
        };

        let payload = serde_json::to_value(request).unwrap();

        assert_eq!(
            payload,
            serde_json::json!({
                "ids": [1, 2],
                "fields": ["System.Title", "System.State"],
                "errorPolicy": "Omit",
            })
        );
    }
}
