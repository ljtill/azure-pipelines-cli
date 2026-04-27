//! HTTP client methods for Azure DevOps Git pull request operations.

use std::future::Future;

use anyhow::Result;

use crate::client::endpoints::pull_requests::PullRequestListRequest;
use crate::client::models::{
    ConnectionData, PullRequest, PullRequestListResponse, PullRequestThread,
    PullRequestThreadListResponse,
};

impl super::AdoClient {
    /// Fetches pull requests across all repositories in the project.
    pub async fn list_pull_requests(
        &self,
        request: PullRequestListRequest<'_>,
    ) -> Result<Vec<PullRequest>> {
        tracing::debug!(?request, "listing pull requests");
        let url = self.endpoints.pull_requests_for_project(request);
        self.get_all_continuation_pages(
            &url,
            "pull_requests",
            None,
            |page: PullRequestListResponse| page.value,
        )
        .await
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
        collect_pull_request_thread_pages(
            &url,
            super::PaginationOptions::new("pull_request_threads", None),
            |full_url| async move {
                self.get_with_continuation::<PullRequestThreadListResponse>(&full_url)
                    .await
            },
        )
        .await
    }

    /// Fetches the connection data to resolve the current user identity.
    pub async fn get_connection_data(&self) -> Result<ConnectionData> {
        tracing::debug!("fetching connection data");
        let url = self.endpoints.connection_data();
        self.get(&url).await
    }
}

async fn collect_pull_request_thread_pages<Fetch, Fut>(
    url: &str,
    options: super::PaginationOptions<'_>,
    fetch_page: Fetch,
) -> Result<Vec<PullRequestThread>>
where
    Fetch: FnMut(String) -> Fut,
    Fut: Future<Output = Result<(PullRequestThreadListResponse, Option<String>)>>,
{
    super::collect_continuation_pages(url, options, fetch_page, |page| page.value).await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::client::errors::AdoError;

    use super::*;

    fn thread(id: u32) -> PullRequestThread {
        PullRequestThread {
            id,
            status: None,
            comments: vec![],
            published_date: None,
            last_updated_date: None,
        }
    }

    #[tokio::test]
    async fn pull_request_thread_pagination_returns_partial_data_at_cap() {
        let calls = Arc::new(AtomicUsize::new(0));

        let err = collect_pull_request_thread_pages(
            "https://example.test/threads?api-version=7.1",
            super::super::PaginationOptions::for_testing("pull_request_threads", 2, None),
            {
                let calls = Arc::clone(&calls);
                move |_| {
                    let calls = Arc::clone(&calls);
                    async move {
                        let id = calls.fetch_add(1, Ordering::Relaxed) as u32;
                        Ok((
                            PullRequestThreadListResponse {
                                value: vec![thread(id)],
                                count: None,
                            },
                            Some("next".to_string()),
                        ))
                    }
                }
            },
        )
        .await
        .expect_err("cap should stop thread pagination");

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
        assert_eq!(*endpoint, "pull_request_threads");
        assert_eq!(*completed_pages, 2);
        assert_eq!(*items, 2);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
}
