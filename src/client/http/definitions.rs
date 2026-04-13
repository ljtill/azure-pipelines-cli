//! HTTP client methods for Azure DevOps pipeline definition operations.

use anyhow::Result;

use crate::client::models::PipelineDefinition;

impl super::AdoClient {
    /// Fetches all pipeline definitions in the configured project, following pagination automatically.
    pub async fn list_definitions(&self) -> Result<Vec<PipelineDefinition>> {
        tracing::debug!("listing pipeline definitions");
        let url = self.endpoints.definitions();
        self.get_all_pages(&url).await
    }
}
