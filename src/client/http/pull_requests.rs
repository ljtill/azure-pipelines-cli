//! HTTP client methods for Azure DevOps Git pull request operations.

use anyhow::Result;

use crate::client::models::{
    ConnectionData, PullRequest, PullRequestListResponse, PullRequestThread,
    PullRequestThreadListResponse,
};

impl super::AdoClient {
    /// Fetches pull requests across all repositories in the project.
    pub async fn list_pull_requests(
        &self,
        status: &str,
        creator_id: Option<&str>,
        reviewer_id: Option<&str>,
    ) -> Result<Vec<PullRequest>> {
        tracing::debug!(status, "listing pull requests");
        let url = self
            .endpoints
            .pull_requests_for_project(status, creator_id, reviewer_id);
        let resp: PullRequestListResponse = self.get(&url).await?;
        Ok(resp.value)
    }

    /// Fetches a single pull request by repository ID and PR ID.
    pub async fn get_pull_request(&self, repo_id: &str, pr_id: u32) -> Result<PullRequest> {
        tracing::debug!(repo_id, pr_id, "getting pull request");
        let url = self.endpoints.pull_request(repo_id, pr_id);
        self.get(&url).await
    }

    /// Fetches comment threads for a pull request.
    pub async fn list_pull_request_threads(
        &self,
        repo_id: &str,
        pr_id: u32,
    ) -> Result<Vec<PullRequestThread>> {
        tracing::debug!(repo_id, pr_id, "listing pull request threads");
        let url = self.endpoints.pull_request_threads(repo_id, pr_id);
        let resp: PullRequestThreadListResponse = self.get(&url).await?;
        Ok(resp.value)
    }

    /// Fetches the connection data to resolve the current user identity.
    pub async fn get_connection_data(&self) -> Result<ConnectionData> {
        tracing::debug!("fetching connection data");
        let url = self.endpoints.connection_data();
        self.get(&url).await
    }
}
