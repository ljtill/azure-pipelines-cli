//! URL builders for the Azure DevOps Git pull requests API.

use super::{Endpoints, encode_path_segment, encode_query_value};

const CONNECTION_DATA_API_VERSION: &str = "7.2-preview.1";
const CONNECTION_DATA_CONNECT_OPTIONS_INCLUDE_SERVICES: u8 = 1;
const CONNECTION_DATA_LAST_CHANGE_ID: i64 = -1;
const TOP_PULL_REQUESTS: u32 = 100;

/// Represents the Azure DevOps pull request status filter values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestStatus {
    /// Matches active pull requests.
    Active,
    /// Matches abandoned pull requests.
    Abandoned,
    /// Matches completed pull requests.
    Completed,
    /// Matches pull requests regardless of status.
    All,
    /// Matches the Azure DevOps `notSet` sentinel status.
    NotSet,
}

impl PullRequestStatus {
    const fn as_query_value(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Abandoned => "abandoned",
            Self::Completed => "completed",
            Self::All => "all",
            Self::NotSet => "notSet",
        }
    }
}

/// Describes filters for listing pull requests across a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullRequestListRequest<'a> {
    status: PullRequestStatus,
    creator_id: Option<&'a str>,
    reviewer_id: Option<&'a str>,
}

impl<'a> PullRequestListRequest<'a> {
    /// Creates a request with the given pull request status filter.
    #[must_use]
    pub const fn new(status: PullRequestStatus) -> Self {
        Self {
            status,
            creator_id: None,
            reviewer_id: None,
        }
    }

    /// Creates a request for active pull requests.
    #[must_use]
    pub const fn active() -> Self {
        Self::new(PullRequestStatus::Active)
    }

    /// Returns a request scoped to pull requests created by the given identity.
    #[must_use]
    pub const fn with_creator_id(mut self, creator_id: &'a str) -> Self {
        self.creator_id = Some(creator_id);
        self
    }

    /// Returns a request scoped to pull requests reviewed by the given identity.
    #[must_use]
    pub const fn with_reviewer_id(mut self, reviewer_id: &'a str) -> Self {
        self.reviewer_id = Some(reviewer_id);
        self
    }
}

impl Endpoints {
    /// Constructs the URL for fetching pull requests across all repositories in the project.
    ///
    /// Supports optional filtering by status, creator ID, and reviewer ID.
    pub fn pull_requests_for_project(&self, request: PullRequestListRequest<'_>) -> String {
        let mut url = format!("{}/git/pullrequests", self.base_url);
        let mut first = true;
        append_query_pair(
            &mut url,
            &mut first,
            "api-version",
            self.api_version.as_ref(),
        );
        append_query_pair(
            &mut url,
            &mut first,
            "searchCriteria.status",
            request.status.as_query_value(),
        );
        let top = TOP_PULL_REQUESTS.to_string();
        append_query_pair(&mut url, &mut first, "$top", &top);
        if let Some(id) = request.creator_id {
            append_query_pair(&mut url, &mut first, "searchCriteria.creatorId", id);
        }
        if let Some(id) = request.reviewer_id {
            append_query_pair(&mut url, &mut first, "searchCriteria.reviewerId", id);
        }
        url
    }

    /// Constructs the URL for fetching a single pull request by repository and PR ID.
    pub fn pull_request(&self, repo_id: &str, pr_id: u32) -> String {
        let repo_id = encode_path_segment(repo_id);
        let mut url = format!(
            "{}/git/repositories/{repo_id}/pullrequests/{pr_id}",
            self.base_url
        );
        let mut first = true;
        append_query_pair(
            &mut url,
            &mut first,
            "api-version",
            self.api_version.as_ref(),
        );
        url
    }

    /// Constructs the URL for fetching comment threads on a pull request.
    pub fn pull_request_threads(&self, repo_id: &str, pr_id: u32) -> String {
        let repo_id = encode_path_segment(repo_id);
        let mut url = format!(
            "{}/git/repositories/{repo_id}/pullrequests/{pr_id}/threads",
            self.base_url
        );
        let mut first = true;
        append_query_pair(
            &mut url,
            &mut first,
            "api-version",
            self.api_version.as_ref(),
        );
        url
    }

    /// Constructs the URL for fetching connection data (current user identity).
    ///
    /// Note: this endpoint is scoped to the organization, not the project.
    pub fn connection_data(&self) -> String {
        // Extract the org base URL from the project-scoped base_url.
        // base_url = "https://dev.azure.com/{org}/{proj}/_apis"
        // We need: "https://dev.azure.com/{org}/_apis/connectionData"
        //
        // Match the Azure DevOps location clients, which send the connection
        // options and sentinel lastChangeId values on this call.
        let org_base = self
            .base_url
            .rsplitn(3, '/')
            .nth(2)
            .unwrap_or(&self.base_url);
        format!(
            "{org_base}/_apis/connectionData?api-version={CONNECTION_DATA_API_VERSION}&connectOptions={CONNECTION_DATA_CONNECT_OPTIONS_INCLUDE_SERVICES}&lastChangeId={CONNECTION_DATA_LAST_CHANGE_ID}&lastChangeId64={CONNECTION_DATA_LAST_CHANGE_ID}"
        )
    }

    /// Constructs the web portal URL for viewing a pull request.
    pub fn web_pull_request(&self, repo_name: &str, pr_id: u32) -> String {
        let repo_name = encode_path_segment(repo_name);
        format!("{}/_git/{repo_name}/pullrequest/{pr_id}", self.web_base_url)
    }
}

fn append_query_pair(url: &mut String, first: &mut bool, key: &str, value: &str) {
    url.push(if *first { '?' } else { '&' });
    *first = false;
    url.push_str(key);
    url.push('=');
    url.push_str(&encode_query_value(value));
}

#[cfg(test)]
mod tests {
    use super::{PullRequestListRequest, PullRequestStatus};
    use crate::client::endpoints::Endpoints;

    fn ep() -> Endpoints {
        Endpoints::new("myorg", "myproj")
    }

    const BASE: &str = "https://dev.azure.com/myorg/myproj/_apis";
    const WEB_BASE: &str = "https://dev.azure.com/myorg/myproj";

    #[test]
    fn pull_requests_for_project_active() {
        let url = ep().pull_requests_for_project(PullRequestListRequest::active());
        assert!(url.starts_with(BASE));
        assert!(url.contains("git/pullrequests"));
        assert!(url.contains("searchCriteria.status=active"));
        assert!(url.contains("$top=100"));
        assert!(!url.contains("creatorId"));
        assert!(!url.contains("reviewerId"));
    }

    #[test]
    fn pull_requests_for_project_with_creator() {
        let url = ep().pull_requests_for_project(
            PullRequestListRequest::active().with_creator_id("user-guid"),
        );
        assert!(url.contains("searchCriteria.creatorId=user-guid"));
    }

    #[test]
    fn pull_requests_for_project_with_reviewer() {
        let url = ep().pull_requests_for_project(
            PullRequestListRequest::active().with_reviewer_id("reviewer-guid"),
        );
        assert!(url.contains("searchCriteria.reviewerId=reviewer-guid"));
    }

    #[test]
    fn pull_requests_for_project_with_both() {
        let url = ep().pull_requests_for_project(
            PullRequestListRequest::active()
                .with_creator_id("creator")
                .with_reviewer_id("reviewer"),
        );
        assert!(url.contains("searchCriteria.creatorId=creator"));
        assert!(url.contains("searchCriteria.reviewerId=reviewer"));
    }

    #[test]
    fn pull_requests_for_project_uses_typed_status_values() {
        let url = ep()
            .pull_requests_for_project(PullRequestListRequest::new(PullRequestStatus::Completed));
        assert!(url.contains("searchCriteria.status=completed"));
    }

    #[test]
    fn pull_requests_for_project_encodes_query_values() {
        let dangerous = "user name&x=y?/ %#\"quotes\"'";
        let url = ep().pull_requests_for_project(
            PullRequestListRequest::active()
                .with_creator_id(dangerous)
                .with_reviewer_id(dangerous),
        );

        let encoded = "user%20name%26x%3Dy%3F%2F%20%25%23%22quotes%22%27";
        assert!(url.contains(&format!("searchCriteria.creatorId={encoded}")));
        assert!(url.contains(&format!("searchCriteria.reviewerId={encoded}")));
        assert!(!url.contains("&x=y"));
        assert!(!url.contains("#\"quotes\""));
    }

    #[test]
    fn pull_requests_for_project_encodes_api_version_override() {
        let mut endpoints = ep();
        endpoints.set_api_version("7.1-preview.1&x=y");
        let url = endpoints.pull_requests_for_project(PullRequestListRequest::active());
        assert!(url.contains("api-version=7.1-preview.1%26x%3Dy"));
        assert!(!url.contains("&x=y"));
    }

    #[test]
    fn pull_request_url() {
        assert_eq!(
            ep().pull_request("repo-guid", 42),
            format!("{BASE}/git/repositories/repo-guid/pullrequests/42?api-version=7.1")
        );
    }

    #[test]
    fn pull_request_url_encodes_repo_id_path_segment() {
        assert_eq!(
            ep().pull_request("repo id/with?bad#part%\"'", 42),
            format!(
                "{BASE}/git/repositories/repo%20id%2Fwith%3Fbad%23part%25%22%27/pullrequests/42?api-version=7.1"
            )
        );
    }

    #[test]
    fn pull_request_threads_url() {
        assert_eq!(
            ep().pull_request_threads("repo-guid", 42),
            format!("{BASE}/git/repositories/repo-guid/pullrequests/42/threads?api-version=7.1")
        );
    }

    #[test]
    fn pull_request_threads_url_encodes_repo_id_path_segment() {
        assert_eq!(
            ep().pull_request_threads("repo id/with?bad#part%\"'", 42),
            format!(
                "{BASE}/git/repositories/repo%20id%2Fwith%3Fbad%23part%25%22%27/pullrequests/42/threads?api-version=7.1"
            )
        );
    }

    #[test]
    fn pull_request_urls_encode_api_version_override() {
        let mut endpoints = ep();
        endpoints.set_api_version("7.1-preview.1&x=y");
        assert_eq!(
            endpoints.pull_request("repo-guid", 42),
            format!(
                "{BASE}/git/repositories/repo-guid/pullrequests/42?api-version=7.1-preview.1%26x%3Dy"
            )
        );
        assert_eq!(
            endpoints.pull_request_threads("repo-guid", 42),
            format!(
                "{BASE}/git/repositories/repo-guid/pullrequests/42/threads?api-version=7.1-preview.1%26x%3Dy"
            )
        );
    }

    #[test]
    fn connection_data_url() {
        let url = ep().connection_data();
        assert_eq!(
            url,
            "https://dev.azure.com/myorg/_apis/connectionData?api-version=7.2-preview.1&connectOptions=1&lastChangeId=-1&lastChangeId64=-1"
        );
    }

    #[test]
    fn web_pull_request_url() {
        assert_eq!(
            ep().web_pull_request("my-repo", 42),
            format!("{WEB_BASE}/_git/my-repo/pullrequest/42")
        );
    }

    #[test]
    fn web_pull_request_encodes_repo_name() {
        let url = ep().web_pull_request("my repo/special", 1);
        assert!(url.contains("my%20repo%2Fspecial"));
    }
}
