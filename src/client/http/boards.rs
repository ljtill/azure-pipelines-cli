//! HTTP client methods for Azure Boards and work item tracking operations.

use std::future::Future;

use anyhow::Result;

use super::RequestRetryPolicy;
use crate::client::models::{
    BacklogLevelConfiguration, BacklogLevelWorkItems, ListResponse, ProjectTeam, WiqlQuery,
    WorkItem, WorkItemBatchGetRequest, WorkItemComment, WorkItemCommentList, WorkItemErrorPolicy,
    WorkItemExpand, WorkItemLink, WorkItemQueryResult, WorkItemTypeCategory,
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
            RequestRetryPolicy::Idempotent,
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
            let resp: ListResponse<WorkItem> = self
                .post_json(&url, &req, RequestRetryPolicy::Idempotent)
                .await?;
            all.extend(resp.value);
        }

        Ok(all)
    }

    /// Fetches a single work item with the extended field set needed by the
    /// detail view. Uses the batch endpoint (one id, field list override) so
    /// we get a predictable payload shape via `WorkItemExpand::Relations`.
    pub async fn get_work_item_detail(&self, id: u32) -> Result<WorkItem> {
        tracing::debug!(work_item_id = id, "fetching work item detail");
        let url = self.endpoints.work_items_batch();
        let req = work_item_batch_request(&[id], &[], Some(WorkItemExpand::Relations));
        let resp: ListResponse<WorkItem> = self
            .post_json(&url, &req, RequestRetryPolicy::Idempotent)
            .await?;
        resp.value
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("work item {id} not found"))
    }

    /// Fetches the comment list for a work item.
    pub async fn list_work_item_comments(&self, id: u32) -> Result<Vec<WorkItemComment>> {
        tracing::debug!(work_item_id = id, "listing work item comments");
        let url = self.endpoints.work_item_comments(id);
        collect_work_item_comment_pages(
            &url,
            super::PaginationOptions::new("work_item_comments", None),
            |full_url| async move { self.get::<WorkItemCommentList>(&full_url).await },
        )
        .await
    }
}

async fn collect_work_item_comment_pages<Fetch, Fut>(
    url: &str,
    options: super::PaginationOptions<'_>,
    mut fetch_page: Fetch,
) -> Result<Vec<WorkItemComment>>
where
    Fetch: FnMut(String) -> Fut,
    Fut: Future<Output = Result<WorkItemCommentList>>,
{
    super::collect_continuation_item_pages(url, options, move |full_url| {
        let fetch = fetch_page(full_url);
        async move {
            let page = fetch.await?;
            Ok((page.comments, page.continuation_token))
        }
    })
    .await
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::client::errors::AdoError;
    use crate::client::http::PaginationOptions;

    use super::*;

    fn comment(id: u32) -> WorkItemComment {
        WorkItemComment {
            id,
            text: format!("comment {id}"),
            created_by: None,
            created_date: None,
            modified_by: None,
            modified_date: None,
        }
    }

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

    #[tokio::test]
    async fn work_item_comment_pagination_returns_partial_data_at_cap() {
        let calls = Arc::new(AtomicUsize::new(0));

        let err = collect_work_item_comment_pages(
            "https://example.test/comments?api-version=7.1-preview.3",
            PaginationOptions::for_testing("work_item_comments", 2, None),
            {
                let calls = Arc::clone(&calls);
                move |_| {
                    let calls = Arc::clone(&calls);
                    async move {
                        let id = calls.fetch_add(1, Ordering::Relaxed) as u32;
                        Ok(WorkItemCommentList {
                            comments: vec![comment(id)],
                            total_count: 10,
                            continuation_token: Some("next".to_string()),
                        })
                    }
                }
            },
        )
        .await
        .expect_err("cap should stop comment pagination");

        let AdoError::PartialData {
            endpoint,
            completed_pages,
            items,
            ..
        } = err
            .downcast_ref::<AdoError>()
            .expect("error should be typed")
        else {
            panic!("expected PartialData");
        };
        assert_eq!(*endpoint, "work_item_comments");
        assert_eq!(*completed_pages, 2);
        assert_eq!(*items, 2);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
}
