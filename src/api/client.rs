use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use reqwest::{Client, Url};

use super::auth::AdoAuth;
use super::endpoints::Endpoints;
use super::models::*;

#[derive(Clone)]
pub struct AdoClient {
    http: Client,
    auth: AdoAuth,
    endpoints: Endpoints,
}

impl AdoClient {
    pub async fn new(organization: &str, project: &str) -> Result<Self> {
        let auth = AdoAuth::new().await?;
        let http = Client::builder()
            .user_agent(concat!("pipelines/", env!("CARGO_PKG_VERSION")))
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
        let display_url = url_without_query(url);
        let start = Instant::now();
        tracing::debug!(method = "GET", url = display_url, "api request");
        let resp = self
            .http
            .get(url)
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?;
        let status = resp.status().as_u16();
        let body = resp.json::<T>().await?;
        tracing::debug!(
            method = "GET",
            url = display_url,
            status,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api response"
        );
        Ok(body)
    }

    async fn get_all_pages<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<Vec<T>> {
        let mut all_items = Vec::new();
        let mut continuation_token: Option<String> = None;
        let mut page_count: u32 = 0;
        let start = Instant::now();

        loop {
            let full_url = paginated_url(url, continuation_token.as_deref())?;

            let token = self.auth.token().await?;
            tracing::debug!(method = "GET", url = %url_without_query(full_url.as_str()), page = page_count + 1, "api paginated request");
            let resp = self
                .http
                .get(full_url)
                .bearer_auth(&token)
                .send()
                .await?
                .error_for_status()?;

            let next_token = resp
                .headers()
                .get("x-ms-continuationtoken")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            let page: ListResponse<T> = resp.json().await?;
            all_items.extend(page.value);
            page_count += 1;

            match next_token {
                Some(t) if !t.is_empty() => continuation_token = Some(t),
                _ => break,
            }
        }

        tracing::debug!(
            method = "GET",
            url = url_without_query(url),
            pages = page_count,
            total_items = all_items.len(),
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api paginated complete"
        );
        Ok(all_items)
    }

    async fn get_text(&self, url: &str) -> Result<String> {
        let token = self.auth.token().await?;
        let display_url = url_without_query(url);
        let start = Instant::now();
        tracing::debug!(method = "GET", url = display_url, "api text request");
        let resp = self
            .http
            .get(url)
            .bearer_auth(&token)
            .send()
            .await?
            .error_for_status()?;
        let status = resp.status().as_u16();
        let text = resp.text().await?;
        tracing::debug!(
            method = "GET",
            url = display_url,
            status,
            bytes = text.len(),
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api text response"
        );
        Ok(text)
    }

    async fn patch_json<B: serde::Serialize>(&self, url: &str, body: &B) -> Result<()> {
        let token = self.auth.token().await?;
        let display_url = url_without_query(url);
        let start = Instant::now();
        tracing::debug!(method = "PATCH", url = display_url, "api request");
        let resp = self
            .http
            .patch(url)
            .bearer_auth(&token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        let status = resp.status().as_u16();
        tracing::debug!(
            method = "PATCH",
            url = display_url,
            status,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api response"
        );
        Ok(())
    }

    async fn post_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<T> {
        let token = self.auth.token().await?;
        let display_url = url_without_query(url);
        let start = Instant::now();
        tracing::debug!(method = "POST", url = display_url, "api request");
        let resp = self
            .http
            .post(url)
            .bearer_auth(&token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        let status = resp.status().as_u16();
        let body = resp.json::<T>().await?;
        tracing::debug!(
            method = "POST",
            url = display_url,
            status,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "api response"
        );
        Ok(body)
    }

    // --- Read operations ---

    pub async fn list_definitions(&self) -> Result<Vec<PipelineDefinition>> {
        tracing::debug!("listing pipeline definitions");
        let url = self.endpoints.definitions();
        self.get_all_pages(&url).await
    }

    pub async fn list_recent_builds(&self) -> Result<Vec<Build>> {
        tracing::debug!("listing recent builds");
        let url = self.endpoints.builds_recent();
        let resp: BuildListResponse = self.get(&url).await?;
        Ok(resp.value)
    }

    pub async fn list_builds_for_definition(&self, definition_id: u32) -> Result<Vec<Build>> {
        tracing::debug!(definition_id, "listing builds for definition");
        let url = self.endpoints.builds_for_definition(definition_id);
        let resp: BuildListResponse = self.get(&url).await?;
        Ok(resp.value)
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

    // --- Write operations ---

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

    pub async fn list_pending_approvals(&self) -> Result<Vec<Approval>> {
        tracing::debug!("listing pending approvals");
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
        tracing::info!(approval_id, status, "updating approval");
        let url = self.endpoints.approvals_update();
        let token = self.auth.token().await?;
        let start = Instant::now();
        let resp = self
            .http
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
        let resp_status = resp.status().as_u16();
        tracing::debug!(
            method = "PATCH",
            status = resp_status,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "approval updated"
        );
        Ok(())
    }
}

fn paginated_url(base_url: &str, continuation_token: Option<&str>) -> Result<Url> {
    let mut url =
        Url::parse(base_url).with_context(|| format!("Failed to parse URL: {base_url}"))?;
    if let Some(token) = continuation_token {
        url.query_pairs_mut()
            .append_pair("continuationToken", token);
    }
    Ok(url)
}

/// Return the URL portion before the query string for logging.
fn url_without_query(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginated_url_preserves_existing_query_params() {
        let url = paginated_url(
            "https://example.test/builds?api-version=7.1&$top=100",
            Some("page-2"),
        )
        .unwrap();

        let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert_eq!(
            query,
            vec![
                ("api-version".into(), "7.1".into()),
                ("$top".into(), "100".into()),
                ("continuationToken".into(), "page-2".into()),
            ]
        );
    }

    #[test]
    fn paginated_url_percent_encodes_opaque_tokens() {
        let url = paginated_url(
            "https://example.test/builds?api-version=7.1",
            Some("abc+/=?&value"),
        )
        .unwrap();

        assert_eq!(
            url.as_str(),
            "https://example.test/builds?api-version=7.1&continuationToken=abc%2B%2F%3D%3F%26value"
        );
    }
}
