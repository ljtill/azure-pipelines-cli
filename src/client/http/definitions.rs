//! HTTP client methods for Azure DevOps pipeline definition operations.

use anyhow::Result;

use crate::client::models::PipelineDefinition;

use super::PaginationProgressFn;

impl super::AdoClient {
    /// Fetches all pipeline definitions in the configured project, following pagination automatically.
    pub async fn list_definitions(&self) -> Result<Vec<PipelineDefinition>> {
        self.list_definitions_with_progress(None).await
    }

    /// Fetches all pipeline definitions, invoking the optional callback with
    /// per-page progress as pagination advances.
    pub async fn list_definitions_with_progress(
        &self,
        progress: Option<&PaginationProgressFn>,
    ) -> Result<Vec<PipelineDefinition>> {
        tracing::debug!("listing pipeline definitions");
        let url = self.endpoints.definitions();
        self.get_all_pages_with_progress(&url, "definitions", progress)
            .await
    }
}
