use anyhow::Result;
use reqwest::Client;

use super::auth::AdoAuth;
use super::endpoints::Endpoints;
use super::models::*;

pub struct AdoClient {
    http: Client,
    auth: AdoAuth,
    pub endpoints: Endpoints,
}

impl AdoClient {
    pub async fn new(organization: &str, project: &str) -> Result<Self> {
        let auth = AdoAuth::new().await?;
        let http = Client::builder()
            .user_agent("pipelines-dashboard/0.1.0")
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
        let resp = self
            .http
            .get(url)
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.text().await?)
    }

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

    pub async fn get_build_timeline(&self, build_id: u32) -> Result<BuildTimeline> {
        let url = self.endpoints.build_timeline(build_id);
        self.get(&url).await
    }

    pub async fn get_build_log(&self, build_id: u32, log_id: u32) -> Result<String> {
        let url = self.endpoints.build_log(build_id, log_id);
        self.get_text(&url).await
    }
}
