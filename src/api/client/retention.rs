use std::collections::HashMap;

use anyhow::Result;

use crate::api::models::*;

impl super::AdoClient {
    pub async fn list_retention_leases_for_definition(
        &self,
        definition_id: u32,
    ) -> Result<Vec<RetentionLease>> {
        tracing::debug!(definition_id, "listing retention leases for definition");
        let url = self
            .endpoints
            .retention_leases_for_definition(definition_id);
        self.get_all_pages(&url).await
    }

    /// Fetch retention leases across multiple definitions in parallel.
    /// Tolerates per-definition failures (logs and skips them).
    pub async fn list_all_retention_leases(
        &self,
        definition_ids: &[u32],
    ) -> Result<Vec<RetentionLease>> {
        if definition_ids.is_empty() {
            return Ok(Vec::new());
        }

        tracing::debug!(
            count = definition_ids.len(),
            "fetching retention leases across definitions"
        );

        let mut set = tokio::task::JoinSet::new();
        for &def_id in definition_ids {
            let client = self.clone();
            set.spawn(async move {
                (
                    def_id,
                    client.list_retention_leases_for_definition(def_id).await,
                )
            });
        }

        let mut all_leases: HashMap<u32, RetentionLease> = HashMap::new();
        let mut failures = 0u32;
        while let Some(result) = set.join_next().await {
            match result {
                Ok((def_id, Ok(leases))) => {
                    for lease in leases {
                        all_leases.entry(lease.lease_id).or_insert(lease);
                    }
                    tracing::trace!(definition_id = def_id, "leases fetched ok");
                }
                Ok((def_id, Err(e))) => {
                    failures += 1;
                    tracing::warn!(definition_id = def_id, error = %e, "failed to fetch leases");
                }
                Err(e) => {
                    failures += 1;
                    tracing::warn!(error = %e, "lease fetch task panicked");
                }
            }
        }

        if failures > 0 {
            tracing::warn!(
                failures,
                total = definition_ids.len(),
                "some lease fetches failed"
            );
        }

        let mut leases: Vec<RetentionLease> = all_leases.into_values().collect();
        leases.sort_by(|a, b| a.lease_id.cmp(&b.lease_id));
        Ok(leases)
    }

    pub async fn delete_retention_leases(&self, ids: &[u32]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        tracing::info!(count = ids.len(), "deleting retention leases");
        let url = self.endpoints.retention_leases_delete(ids);
        self.delete(&url).await
    }
}
