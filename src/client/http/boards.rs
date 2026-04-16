//! HTTP client methods for Azure Boards and work item tracking operations.

use anyhow::Result;

use crate::client::models::{
    BacklogLevelConfiguration, BacklogLevelWorkItems, ListResponse, ProjectTeam, WiqlQuery,
    WorkItem, WorkItemBatchGetRequest, WorkItemErrorPolicy, WorkItemExpand, WorkItemLink,
    WorkItemQueryResult, WorkItemTypeCategory,
};

impl super::AdoClient {
    /// Fetches all project teams.
    pub async fn list_project_teams(&self) -> Result<Vec<ProjectTeam>> {
        tracing::debug!("listing project teams");
        let url = self.endpoints.project_teams();
        let resp: ListResponse<ProjectTeam> = self.get(&url).await?;
        Ok(resp.value)
    }

    /// Fetches backlog metadata for the given team.
    pub async fn list_backlogs(&self, team: &str) -> Result<Vec<BacklogLevelConfiguration>> {
        tracing::debug!(team, "listing backlogs");
        let url = self.endpoints.backlogs(team);
        let resp: ListResponse<BacklogLevelConfiguration> = self.get(&url).await?;
        Ok(resp.value)
    }

    /// Fetches work item links for a backlog level.
    pub async fn list_backlog_level_work_items(
        &self,
        team: &str,
        backlog_id: &str,
    ) -> Result<Vec<WorkItemLink>> {
        tracing::debug!(team, backlog_id, "listing backlog work items");
        let url = self.endpoints.backlog_level_work_items(team, backlog_id);
        let resp: BacklogLevelWorkItems = self.get(&url).await?;
        Ok(resp.work_items)
    }

    /// Fetches project work item type categories.
    pub async fn list_work_item_type_categories(&self) -> Result<Vec<WorkItemTypeCategory>> {
        tracing::debug!("listing work item type categories");
        let url = self.endpoints.work_item_type_categories();
        let resp: ListResponse<WorkItemTypeCategory> = self.get(&url).await?;
        Ok(resp.value)
    }

    /// Executes a WIQL query and returns the query result.
    pub async fn query_by_wiql(&self, query: &str) -> Result<WorkItemQueryResult> {
        tracing::debug!("querying work items by WIQL");
        let url = self.endpoints.wiql();
        self.post_json(
            &url,
            &WiqlQuery {
                query: query.to_string(),
            },
        )
        .await
    }

    /// Fetches work items in batches of up to 200 IDs.
    pub async fn get_work_items_batch(
        &self,
        ids: &[u32],
        fields: &[&str],
        expand: Option<WorkItemExpand>,
    ) -> Result<Vec<WorkItem>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let url = self.endpoints.work_items_batch();
        let mut all = Vec::new();

        for chunk in ids.chunks(200) {
            tracing::debug!(count = chunk.len(), "fetching work item batch");
            let req = work_item_batch_request(chunk, fields, expand);
            let resp: ListResponse<WorkItem> = self.post_json(&url, &req).await?;
            all.extend(resp.value);
        }

        Ok(all)
    }
}

fn work_item_batch_request(
    ids: &[u32],
    fields: &[&str],
    expand: Option<WorkItemExpand>,
) -> WorkItemBatchGetRequest {
    let fields = if matches!(expand, Some(WorkItemExpand::Relations)) {
        Vec::new()
    } else {
        fields.iter().map(|field| (*field).to_string()).collect()
    };

    WorkItemBatchGetRequest {
        ids: ids.to_vec(),
        fields,
        expand,
        error_policy: Some(WorkItemErrorPolicy::Omit),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boards_batch_contract_uses_org_scoped_url_and_fields_only_body() {
        let url =
            crate::client::endpoints::Endpoints::new("my org", "my project").work_items_batch();
        let payload = serde_json::to_value(work_item_batch_request(
            &[1, 2],
            &["System.Title", "System.State"],
            None,
        ))
        .unwrap();

        assert_eq!(
            url,
            "https://dev.azure.com/my%20org/_apis/wit/workitemsbatch?api-version=7.1"
        );
        assert_eq!(
            payload,
            serde_json::json!({
                "ids": [1, 2],
                "fields": ["System.Title", "System.State"],
                "errorPolicy": "Omit",
            })
        );
    }

    #[test]
    fn boards_batch_contract_omits_fields_when_requesting_relations_expand() {
        let payload = serde_json::to_value(work_item_batch_request(
            &[1, 2],
            &["System.Title", "System.State"],
            Some(WorkItemExpand::Relations),
        ))
        .unwrap();

        assert_eq!(
            payload,
            serde_json::json!({
                "ids": [1, 2],
                "$expand": "Relations",
                "errorPolicy": "Omit",
            })
        );
    }
}
