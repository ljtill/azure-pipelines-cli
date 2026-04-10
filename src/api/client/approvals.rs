use std::time::Instant;

use anyhow::Result;

use crate::api::models::*;

impl super::AdoClient {
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
