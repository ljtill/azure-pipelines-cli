use std::time::Duration;

use anyhow::Result;
use reqwest::Client;

use super::auth::AdoAuth;
use super::endpoints::Endpoints;
use super::models::*;

#[derive(Clone)]
pub struct AdoClient {
    http: Client,
    auth: AdoAuth,
    pub endpoints: Endpoints,
}

impl AdoClient {
    pub async fn new(organization: &str, project: &str) -> Result<Self> {
        let auth = AdoAuth::new().await?;
        let http = Client::builder()
            .user_agent(concat!("azure-pipelines-cli/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        let endpoints = Endpoints::new(organization, project);

        Ok(Self {
            http,
            auth,
            endpoints,
        })
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let token = self.auth.token().await?;
        tracing::debug!(method = "GET", url, "api request");
        let resp = self
            .http
            .get(url)
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?;
        let body = resp.json::<T>().await?;
        Ok(body)
    }

    async fn get_text(&self, url: &str) -> Result<String> {
        let token = self.auth.token().await?;
        tracing::debug!(method = "GET", url, "api text request");
        let resp = self
            .http
            .get(url)
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.text().await?)
    }

    async fn patch_json<B: serde::Serialize>(&self, url: &str, body: &B) -> Result<()> {
        let token = self.auth.token().await?;
        tracing::debug!(method = "PATCH", url, "api request");
        self.http
            .patch(url)
            .bearer_auth(&token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn post_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<T> {
        let token = self.auth.token().await?;
        tracing::debug!(method = "POST", url, "api request");
        let resp = self
            .http
            .post(url)
            .bearer_auth(&token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json::<T>().await?)
    }

    // --- Read operations ---

    pub async fn list_definitions(&self) -> Result<Vec<PipelineDefinition>> {
        let url = self.endpoints.definitions();
        let resp: DefinitionListResponse = self.get(&url).await?;
        Ok(resp.value)
    }

    pub async fn list_active_builds(&self) -> Result<Vec<Build>> {
        let url = self.endpoints.builds_active();
        let resp: BuildListResponse = self.get(&url).await?;
        Ok(resp.value)
    }

    pub async fn list_recent_builds(&self) -> Result<Vec<Build>> {
        let url = self.endpoints.builds_recent();
        let resp: BuildListResponse = self.get(&url).await?;
        Ok(resp.value)
    }

    pub async fn list_builds_for_definition(&self, definition_id: u32) -> Result<Vec<Build>> {
        let url = self.endpoints.builds_for_definition(definition_id);
        let resp: BuildListResponse = self.get(&url).await?;
        Ok(resp.value)
    }

    pub async fn get_build(&self, build_id: u32) -> Result<Build> {
        let url = self.endpoints.build(build_id);
        self.get(&url).await
    }

    pub async fn get_build_timeline(&self, build_id: u32) -> Result<BuildTimeline> {
        let url = self.endpoints.build_timeline(build_id);
        self.get(&url).await
    }

    pub async fn get_build_log(&self, build_id: u32, log_id: u32) -> Result<String> {
        let url = self.endpoints.build_log(build_id, log_id);
        self.get_text(&url).await
    }

    // --- Write operations ---

    pub async fn cancel_build(&self, build_id: u32) -> Result<()> {
        let url = self.endpoints.build(build_id);
        self.patch_json(&url, &serde_json::json!({"status": "cancelling"}))
            .await
    }

    pub async fn retry_stage(&self, build_id: u32, stage_ref_name: &str) -> Result<()> {
        let url = self.endpoints.build_stage(build_id, stage_ref_name);
        self.patch_json(
            &url,
            &serde_json::json!({"forceRetryAllJobs": true, "state": 1}),
        )
        .await
    }

    pub async fn run_pipeline(&self, pipeline_id: u32) -> Result<PipelineRun> {
        let url = self.endpoints.pipeline_runs(pipeline_id);
        self.post_json(&url, &serde_json::json!({})).await
    }

    pub async fn list_pending_approvals(&self) -> Result<Vec<Approval>> {
        let url = self.endpoints.approvals_pending();
        let resp: ApprovalListResponse = self.get(&url).await?;
        Ok(resp.value)
    }

    pub async fn update_approval(
        &self,
        approval_id: &str,
        status: &str,
        comment: &str,
    ) -> Result<()> {
        let url = self.endpoints.approvals_update();
        let token = self.auth.token().await?;
        self.http
            .patch(&url)
            .bearer_auth(&token)
            .json(&serde_json::json!([{
                "approvalId": approval_id,
                "status": status,
                "comment": comment
            }]))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}
