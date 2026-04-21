//! HTTP client methods for Azure DevOps retention lease operations.

use std::collections::HashMap;

use anyhow::Result;

use crate::client::models::RetentionLease;

impl super::AdoClient {
    /// Fetches retention leases for a single pipeline definition.
    pub async fn list_retention_leases_for_definition(
        &self,
        definition_id: u32,
    ) -> Result<Vec<RetentionLease>> {
        self.list_retention_leases_for_definition_with_progress(definition_id, None)
            .await
    }

    /// Fetches retention leases for a single pipeline definition, invoking the
    /// optional callback with per-page progress as pagination advances.
    pub async fn list_retention_leases_for_definition_with_progress(
        &self,
        definition_id: u32,
        progress: Option<&super::PaginationProgressFn>,
    ) -> Result<Vec<RetentionLease>> {
        tracing::debug!(definition_id, "listing retention leases for definition");
        let url = self
            .endpoints
            .retention_leases_for_definition(definition_id);
        self.get_all_pages_with_progress(&url, "retention_leases", progress)
            .await
    }

    /// Fetches retention leases across multiple definitions in parallel.
    /// Tolerates per-definition failures (logs and skips them).
    pub async fn list_all_retention_leases(
        &self,
        definition_ids: &[u32],
    ) -> Result<Vec<RetentionLease>> {
        self.list_all_retention_leases_with_progress(definition_ids, None)
            .await
    }

    /// Fetches retention leases across multiple definitions in parallel,
    /// invoking the optional callback with aggregated progress as each
    /// definition's fetch completes.
    ///
    /// The callback receives the cumulative count of definitions processed as
    /// the `page` value and the total number of leases accumulated so far —
    /// this maps cleanly onto the UI's per-page progress surface without
    /// requiring per-page events from every parallel fetcher to interleave.
    pub async fn list_all_retention_leases_with_progress(
        &self,
        definition_ids: &[u32],
        progress: Option<&super::PaginationProgressFn>,
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
        let mut completed: usize = 0;
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
            completed += 1;
            if let Some(cb) = progress {
                cb(super::PaginationProgress {
                    endpoint: "retention_leases",
                    page: completed,
                    items_so_far: all_leases.len(),
                });
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
        leases.sort_by_key(|l| l.lease_id);
        Ok(leases)
    }

    /// Deletes the specified retention leases by their IDs.
    pub async fn delete_retention_leases(&self, ids: &[u32]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        tracing::info!(count = ids.len(), "deleting retention leases");
        let url = self.endpoints.retention_leases_delete(ids);
        self.delete(&url).await
    }
}
