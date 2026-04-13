//! URL builders for the Azure DevOps Git pull requests API.

use super::{API_VERSION, Endpoints};

const CONNECTION_DATA_API_VERSION: &str = "7.2-preview.1";
const CONNECTION_DATA_CONNECT_OPTIONS_INCLUDE_SERVICES: u8 = 1;
const CONNECTION_DATA_LAST_CHANGE_ID: i64 = -1;
const TOP_PULL_REQUESTS: u32 = 100;

impl Endpoints {
    /// Constructs the URL for fetching pull requests across all repositories in the project.
    ///
    /// Supports optional filtering by status, creator ID, and reviewer ID.
    pub fn pull_requests_for_project(
        &self,
        status: &str,
        creator_id: Option<&str>,
        reviewer_id: Option<&str>,
    ) -> String {
        let mut url = format!(
            "{}/git/pullrequests?api-version={API_VERSION}&searchCriteria.status={status}&$top={TOP_PULL_REQUESTS}",
            self.base_url
        );
        if let Some(id) = creator_id {
            use std::fmt::Write;
            write!(url, "&searchCriteria.creatorId={id}").unwrap();
        }
        if let Some(id) = reviewer_id {
            use std::fmt::Write;
            write!(url, "&searchCriteria.reviewerId={id}").unwrap();
        }
        url
    }

    /// Constructs the URL for fetching a single pull request by repository and PR ID.
    pub fn pull_request(&self, repo_id: &str, pr_id: u32) -> String {
        format!(
            "{}/git/repositories/{repo_id}/pullrequests/{pr_id}?api-version={API_VERSION}",
            self.base_url
        )
    }

    /// Constructs the URL for fetching comment threads on a pull request.
    pub fn pull_request_threads(&self, repo_id: &str, pr_id: u32) -> String {
        format!(
            "{}/git/repositories/{repo_id}/pullrequests/{pr_id}/threads?api-version={API_VERSION}",
            self.base_url
        )
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
        let repo_name = super::encode_path_segment(repo_name);
        format!("{}/_git/{repo_name}/pullrequest/{pr_id}", self.web_base_url)
    }
}

#[cfg(test)]
mod tests {
    use crate::client::endpoints::Endpoints;

    fn ep() -> Endpoints {
        Endpoints::new("myorg", "myproj")
    }

    const BASE: &str = "https://dev.azure.com/myorg/myproj/_apis";
    const WEB_BASE: &str = "https://dev.azure.com/myorg/myproj";

    #[test]
    fn pull_requests_for_project_active() {
        let url = ep().pull_requests_for_project("active", None, None);
        assert!(url.starts_with(BASE));
        assert!(url.contains("git/pullrequests"));
        assert!(url.contains("searchCriteria.status=active"));
        assert!(url.contains("$top=100"));
        assert!(!url.contains("creatorId"));
        assert!(!url.contains("reviewerId"));
    }

    #[test]
    fn pull_requests_for_project_with_creator() {
        let url = ep().pull_requests_for_project("active", Some("user-guid"), None);
        assert!(url.contains("searchCriteria.creatorId=user-guid"));
    }

    #[test]
    fn pull_requests_for_project_with_reviewer() {
        let url = ep().pull_requests_for_project("active", None, Some("reviewer-guid"));
        assert!(url.contains("searchCriteria.reviewerId=reviewer-guid"));
    }

    #[test]
    fn pull_requests_for_project_with_both() {
        let url = ep().pull_requests_for_project("active", Some("creator"), Some("reviewer"));
        assert!(url.contains("searchCriteria.creatorId=creator"));
        assert!(url.contains("searchCriteria.reviewerId=reviewer"));
    }

    #[test]
    fn pull_request_url() {
        assert_eq!(
            ep().pull_request("repo-guid", 42),
            format!("{BASE}/git/repositories/repo-guid/pullrequests/42?api-version=7.1")
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
