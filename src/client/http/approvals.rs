//! HTTP client methods for Azure DevOps pipeline approval operations.

use anyhow::Result;

use super::RequestRetryPolicy;
use crate::client::models::{Approval, ApprovalListResponse};

impl super::AdoClient {
    /// Fetches all pending approvals for the configured project.
    pub async fn list_pending_approvals(&self) -> Result<Vec<Approval>> {
        tracing::debug!("listing pending approvals");
        let url = self.endpoints.approvals_pending();
        self.get_all_continuation_pages(&url, "approvals", None, |page: ApprovalListResponse| {
            page.value
        })
        .await
    }

    /// Sends an approval status update (approve/reject) with an optional comment.
    pub async fn update_approval(
        &self,
        approval_id: &str,
        status: &str,
        comment: &str,
    ) -> Result<()> {
        tracing::info!(approval_id, status, "updating approval");
        let url = self.endpoints.approvals_update();
        self.patch_json(
            &url,
            &serde_json::json!([{
                "approvalId": approval_id,
                "status": status,
                "comment": comment
            }]),
            RequestRetryPolicy::NonIdempotent,
        )
        .await
    }
}
