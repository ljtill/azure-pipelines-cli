use anyhow::Result;

use super::encode_continuation_token;
use crate::api::models::*;

impl super::AdoClient {
    pub async fn list_recent_builds(&self) -> Result<Vec<Build>> {
        tracing::debug!("listing recent builds");
        let url = self.endpoints.builds_recent();
        let resp: BuildListResponse = self.get(&url).await?;
        Ok(resp.value)
    }

    pub async fn list_builds_for_definition(
        &self,
        definition_id: u32,
    ) -> Result<(Vec<Build>, Option<String>)> {
        tracing::debug!(definition_id, "listing builds for definition");
        let url = self.endpoints.builds_for_definition(definition_id);
        let (resp, continuation): (BuildListResponse, _) = self.get_with_continuation(&url).await?;
        Ok((resp.value, continuation))
    }

    pub async fn list_builds_for_definition_continued(
        &self,
        definition_id: u32,
        continuation_token: &str,
    ) -> Result<(Vec<Build>, Option<String>)> {
        tracing::debug!(definition_id, "listing builds for definition (continued)");
        let base_url = self.endpoints.builds_for_definition(definition_id);
        let url = format!(
            "{}&continuationToken={}",
            base_url,
            encode_continuation_token(continuation_token)
        );
        let (resp, continuation): (BuildListResponse, _) = self.get_with_continuation(&url).await?;
        Ok((resp.value, continuation))
    }

    pub async fn get_build(&self, build_id: u32) -> Result<Build> {
        tracing::debug!(build_id, "getting build");
        let url = self.endpoints.build(build_id);
        self.get(&url).await
    }

    pub async fn get_build_timeline(&self, build_id: u32) -> Result<BuildTimeline> {
        tracing::debug!(build_id, "getting build timeline");
        let url = self.endpoints.build_timeline(build_id);
        self.get(&url).await
    }

    pub async fn get_build_log(&self, build_id: u32, log_id: u32) -> Result<String> {
        tracing::debug!(build_id, log_id, "getting build log");
        let url = self.endpoints.build_log(build_id, log_id);
        self.get_text(&url).await
    }

    pub async fn cancel_build(&self, build_id: u32) -> Result<()> {
        tracing::info!(build_id, "cancelling build");
        let url = self.endpoints.build(build_id);
        self.patch_json(&url, &serde_json::json!({"status": "cancelling"}))
            .await
    }

    pub async fn retry_stage(&self, build_id: u32, stage_ref_name: &str) -> Result<()> {
        tracing::info!(build_id, stage = stage_ref_name, "retrying stage");
        let url = self.endpoints.build_stage(build_id, stage_ref_name);
        self.patch_json(
            &url,
            &serde_json::json!({"forceRetryAllJobs": true, "state": 1}),
        )
        .await
    }

    pub async fn run_pipeline(&self, pipeline_id: u32) -> Result<PipelineRun> {
        tracing::info!(pipeline_id, "running pipeline");
        let url = self.endpoints.pipeline_runs(pipeline_id);
        self.post_json(&url, &serde_json::json!({})).await
    }
}
