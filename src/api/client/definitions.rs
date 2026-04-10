use anyhow::Result;

use crate::api::models::*;

impl super::AdoClient {
    pub async fn list_definitions(&self) -> Result<Vec<PipelineDefinition>> {
        tracing::debug!("listing pipeline definitions");
        let url = self.endpoints.definitions();
        self.get_all_pages(&url).await
    }
}
