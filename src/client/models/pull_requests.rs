//! Azure DevOps Git pull request model types.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::builds::IdentityRef;

// --- Pull Requests ---

/// Represents a paginated list of pull requests.
#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestListResponse {
    pub value: Vec<PullRequest>,
    #[allow(dead_code)]
    pub count: Option<u32>,
}

/// Represents a single Azure DevOps pull request.
#[derive(Debug, Clone, Deserialize)]
pub struct PullRequest {
    #[serde(rename = "pullRequestId")]
    pub pull_request_id: u32,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    #[serde(rename = "createdBy")]
    pub created_by: Option<IdentityRef>,
    #[serde(rename = "creationDate")]
    pub creation_date: Option<DateTime<Utc>>,
    #[serde(rename = "sourceRefName")]
    pub source_ref_name: Option<String>,
    #[serde(rename = "targetRefName")]
    pub target_ref_name: Option<String>,
    pub repository: Option<GitRepositoryRef>,
    #[serde(default)]
    pub reviewers: Vec<Reviewer>,
    #[serde(rename = "mergeStatus")]
    pub merge_status: Option<String>,
    #[serde(rename = "isDraft", default)]
    pub is_draft: bool,
    pub url: Option<String>,
    #[serde(default)]
    pub labels: Vec<PullRequestLabel>,
}

impl PullRequest {
    /// Returns the source branch name with `refs/heads/` prefix stripped.
    pub fn short_source_branch(&self) -> &str {
        self.source_ref_name
            .as_deref()
            .unwrap_or("")
            .strip_prefix("refs/heads/")
            .unwrap_or_else(|| self.source_ref_name.as_deref().unwrap_or(""))
    }

    /// Returns the target branch name with `refs/heads/` prefix stripped.
    pub fn short_target_branch(&self) -> &str {
        self.target_ref_name
            .as_deref()
            .unwrap_or("")
            .strip_prefix("refs/heads/")
            .unwrap_or_else(|| self.target_ref_name.as_deref().unwrap_or(""))
    }

    /// Returns the display name of the PR author.
    pub fn author(&self) -> &str {
        self.created_by
            .as_ref()
            .map_or("Unknown", |r| r.display_name.as_str())
    }

    /// Returns the repository name, or an empty string if unavailable.
    pub fn repo_name(&self) -> &str {
        self.repository.as_ref().map_or("", |r| r.name.as_str())
    }

    /// Returns `true` if the pull request status is "active" (case-insensitive).
    pub fn is_active(&self) -> bool {
        self.status.eq_ignore_ascii_case("active")
    }

    /// Returns a summary of reviewer votes as (approved, rejected, waiting, no_vote).
    pub fn vote_summary(&self) -> (usize, usize, usize, usize) {
        let mut approved = 0;
        let mut rejected = 0;
        let mut waiting = 0;
        let mut no_vote = 0;
        for r in &self.reviewers {
            match r.vote {
                10 | 5 => approved += 1,
                -10 => rejected += 1,
                -5 => waiting += 1,
                _ => no_vote += 1,
            }
        }
        (approved, rejected, waiting, no_vote)
    }
}

/// Represents a minimal Git repository reference embedded in PR responses.
#[derive(Debug, Clone, Deserialize)]
pub struct GitRepositoryRef {
    pub id: String,
    pub name: String,
}

/// Represents a pull request reviewer with a vote.
#[derive(Debug, Clone, Deserialize)]
pub struct Reviewer {
    pub id: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "uniqueName")]
    pub unique_name: Option<String>,
    #[serde(default)]
    pub vote: i32,
    #[serde(rename = "isRequired", default)]
    pub is_required: bool,
    #[serde(rename = "hasDeclined", default)]
    pub has_declined: bool,
}

// --- Pull Request Threads ---

/// Represents a comment thread on a pull request.
#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestThread {
    pub id: u32,
    pub status: Option<String>,
    #[serde(default)]
    pub comments: Vec<PullRequestComment>,
    #[serde(rename = "publishedDate")]
    pub published_date: Option<DateTime<Utc>>,
    #[serde(rename = "lastUpdatedDate")]
    pub last_updated_date: Option<DateTime<Utc>>,
}

impl PullRequestThread {
    /// Returns `true` if the thread status is "active" or "pending" (case-insensitive).
    pub fn is_active(&self) -> bool {
        self.status
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case("active") || s.eq_ignore_ascii_case("pending"))
    }
}

/// Represents a paginated list of pull request threads.
#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestThreadListResponse {
    pub value: Vec<PullRequestThread>,
    #[allow(dead_code)]
    pub count: Option<u32>,
}

/// Represents a single comment within a thread.
#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestComment {
    pub id: u32,
    pub author: Option<IdentityRef>,
    pub content: Option<String>,
    #[serde(rename = "publishedDate")]
    pub published_date: Option<DateTime<Utc>>,
    #[serde(rename = "commentType")]
    pub comment_type: Option<String>,
}

/// Represents a label attached to a pull request.
#[derive(Debug, Clone, Deserialize)]
pub struct PullRequestLabel {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub active: bool,
}

// --- Connection Data ---

/// Represents the connection data response used to resolve the current user identity.
#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionData {
    #[serde(rename = "authenticatedUser")]
    pub authenticated_user: Option<AuthenticatedUser>,
}

/// Represents the authenticated user within a connection data response.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthenticatedUser {
    pub id: String,
    #[serde(rename = "providerDisplayName")]
    pub provider_display_name: Option<String>,
}

impl ConnectionData {
    /// Extracts the authenticated user's ID.
    pub fn user_id(&self) -> Option<&str> {
        self.authenticated_user.as_ref().map(|u| u.id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- PullRequest helper methods ---

    #[test]
    fn short_source_branch_strips_prefix() {
        let pr = PullRequest {
            pull_request_id: 1,
            title: "Test".to_string(),
            description: None,
            status: "active".to_string(),
            created_by: None,
            creation_date: None,
            source_ref_name: Some("refs/heads/feat/widget".to_string()),
            target_ref_name: Some("refs/heads/main".to_string()),
            repository: None,
            reviewers: vec![],
            merge_status: None,
            is_draft: false,
            url: None,
            labels: vec![],
        };
        assert_eq!(pr.short_source_branch(), "feat/widget");
        assert_eq!(pr.short_target_branch(), "main");
    }

    #[test]
    fn author_returns_display_name() {
        let pr = PullRequest {
            pull_request_id: 1,
            title: "Test".to_string(),
            description: None,
            status: "active".to_string(),
            created_by: Some(IdentityRef {
                display_name: "Alice".to_string(),
            }),
            creation_date: None,
            source_ref_name: None,
            target_ref_name: None,
            repository: None,
            reviewers: vec![],
            merge_status: None,
            is_draft: false,
            url: None,
            labels: vec![],
        };
        assert_eq!(pr.author(), "Alice");
    }

    #[test]
    fn author_unknown_when_none() {
        let pr = PullRequest {
            pull_request_id: 1,
            title: "Test".to_string(),
            description: None,
            status: "active".to_string(),
            created_by: None,
            creation_date: None,
            source_ref_name: None,
            target_ref_name: None,
            repository: None,
            reviewers: vec![],
            merge_status: None,
            is_draft: false,
            url: None,
            labels: vec![],
        };
        assert_eq!(pr.author(), "Unknown");
    }

    #[test]
    fn is_active_case_insensitive() {
        let make = |status: &str| PullRequest {
            pull_request_id: 1,
            title: "T".to_string(),
            description: None,
            status: status.to_string(),
            created_by: None,
            creation_date: None,
            source_ref_name: None,
            target_ref_name: None,
            repository: None,
            reviewers: vec![],
            merge_status: None,
            is_draft: false,
            url: None,
            labels: vec![],
        };
        assert!(make("active").is_active());
        assert!(make("Active").is_active());
        assert!(!make("completed").is_active());
        assert!(!make("abandoned").is_active());
    }

    #[test]
    fn vote_summary_counts() {
        let pr = PullRequest {
            pull_request_id: 1,
            title: "T".to_string(),
            description: None,
            status: "active".to_string(),
            created_by: None,
            creation_date: None,
            source_ref_name: None,
            target_ref_name: None,
            repository: None,
            reviewers: vec![
                Reviewer {
                    id: None,
                    display_name: "Alice".to_string(),
                    unique_name: None,
                    vote: 10,
                    is_required: false,
                    has_declined: false,
                },
                Reviewer {
                    id: None,
                    display_name: "Bob".to_string(),
                    unique_name: None,
                    vote: -10,
                    is_required: false,
                    has_declined: true,
                },
                Reviewer {
                    id: None,
                    display_name: "Carol".to_string(),
                    unique_name: None,
                    vote: -5,
                    is_required: true,
                    has_declined: false,
                },
                Reviewer {
                    id: None,
                    display_name: "Dave".to_string(),
                    unique_name: None,
                    vote: 0,
                    is_required: false,
                    has_declined: false,
                },
                Reviewer {
                    id: None,
                    display_name: "Eve".to_string(),
                    unique_name: None,
                    vote: 5,
                    is_required: false,
                    has_declined: false,
                },
            ],
            merge_status: None,
            is_draft: false,
            url: None,
            labels: vec![],
        };
        let (approved, rejected, waiting, no_vote) = pr.vote_summary();
        assert_eq!(approved, 2); // Alice (10) + Eve (5)
        assert_eq!(rejected, 1); // Bob (-10)
        assert_eq!(waiting, 1); // Carol (-5)
        assert_eq!(no_vote, 1); // Dave (0)
    }

    // --- Deserialization ---

    #[test]
    fn deserialize_pull_request() {
        let json = r#"{
            "pullRequestId": 42,
            "title": "Add feature X",
            "description": "This adds feature X",
            "status": "active",
            "createdBy": {"displayName": "Alice"},
            "creationDate": "2024-06-15T12:00:00Z",
            "sourceRefName": "refs/heads/feat/x",
            "targetRefName": "refs/heads/main",
            "repository": {"id": "repo-guid", "name": "my-repo"},
            "reviewers": [
                {"displayName": "Bob", "vote": 10, "isRequired": true}
            ],
            "mergeStatus": "succeeded",
            "isDraft": false,
            "labels": [{"name": "bug", "active": true}]
        }"#;
        let pr: PullRequest = serde_json::from_str(json).unwrap();
        assert_eq!(pr.pull_request_id, 42);
        assert_eq!(pr.title, "Add feature X");
        assert_eq!(pr.status, "active");
        assert_eq!(pr.author(), "Alice");
        assert_eq!(pr.short_source_branch(), "feat/x");
        assert_eq!(pr.short_target_branch(), "main");
        assert_eq!(pr.repo_name(), "my-repo");
        assert!(pr.is_active());
        assert!(!pr.is_draft);
        assert_eq!(pr.reviewers.len(), 1);
        assert_eq!(pr.reviewers[0].vote, 10);
        assert!(pr.reviewers[0].is_required);
        assert_eq!(pr.labels.len(), 1);
        assert_eq!(pr.labels[0].name, "bug");
    }

    #[test]
    fn deserialize_pull_request_minimal() {
        let json = r#"{
            "pullRequestId": 1,
            "title": "Minimal PR",
            "status": "completed"
        }"#;
        let pr: PullRequest = serde_json::from_str(json).unwrap();
        assert_eq!(pr.pull_request_id, 1);
        assert!(!pr.is_active());
        assert!(pr.reviewers.is_empty());
        assert!(pr.labels.is_empty());
        assert!(!pr.is_draft);
    }

    #[test]
    fn deserialize_thread() {
        let json = r#"{
            "id": 10,
            "status": "active",
            "comments": [
                {
                    "id": 1,
                    "author": {"displayName": "Alice"},
                    "content": "Looks good!",
                    "publishedDate": "2024-06-15T13:00:00Z",
                    "commentType": "text"
                }
            ],
            "publishedDate": "2024-06-15T12:30:00Z"
        }"#;
        let thread: PullRequestThread = serde_json::from_str(json).unwrap();
        assert_eq!(thread.id, 10);
        assert!(thread.is_active());
        assert_eq!(thread.comments.len(), 1);
        assert_eq!(thread.comments[0].content.as_deref(), Some("Looks good!"));
    }

    #[test]
    fn deserialize_connection_data() {
        let json = r#"{
            "authenticatedUser": {
                "id": "user-guid-123",
                "providerDisplayName": "Alice Smith"
            }
        }"#;
        let cd: ConnectionData = serde_json::from_str(json).unwrap();
        assert_eq!(cd.user_id(), Some("user-guid-123"));
        assert_eq!(
            cd.authenticated_user.unwrap().provider_display_name,
            Some("Alice Smith".to_string())
        );
    }

    #[test]
    fn thread_is_active_pending() {
        let thread = PullRequestThread {
            id: 1,
            status: Some("pending".to_string()),
            comments: vec![],
            published_date: None,
            last_updated_date: None,
        };
        assert!(thread.is_active());
    }

    #[test]
    fn thread_is_active_closed() {
        let thread = PullRequestThread {
            id: 1,
            status: Some("closed".to_string()),
            comments: vec![],
            published_date: None,
            last_updated_date: None,
        };
        assert!(!thread.is_active());
    }
}
