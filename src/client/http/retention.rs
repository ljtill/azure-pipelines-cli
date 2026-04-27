//! HTTP client methods for Azure DevOps retention lease operations.

use std::collections::HashMap;
use std::future::Future;

use anyhow::Result;

use super::RequestRetryPolicy;
use crate::client::models::RetentionLease;
use crate::shared::concurrency::{API_FAN_OUT_LIMIT, for_each_bounded};

/// Represents a failed retention lease fetch for one definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionLeaseFetchFailure {
    pub definition_id: Option<u32>,
    pub message: String,
}

/// Represents retention leases fetched across definitions, including partial failures.
#[derive(Debug, Clone, Default)]
pub struct RetentionLeaseFetchResult {
    pub leases: Vec<RetentionLease>,
    pub failures: Vec<RetentionLeaseFetchFailure>,
}

impl RetentionLeaseFetchResult {
    /// Returns whether any definition failed while fetching retention leases.
    pub fn is_partial(&self) -> bool {
        !self.failures.is_empty()
    }
}

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
        Ok(self
            .list_all_retention_leases_with_progress(definition_ids, None)
            .await?
            .leases)
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
    ) -> Result<RetentionLeaseFetchResult> {
        Ok(self
            .list_all_retention_leases_with_progress_limit(
                definition_ids,
                progress,
                API_FAN_OUT_LIMIT,
            )
            .await)
    }

    async fn list_all_retention_leases_with_progress_limit(
        &self,
        definition_ids: &[u32],
        progress: Option<&super::PaginationProgressFn>,
        limit: usize,
    ) -> RetentionLeaseFetchResult {
        if definition_ids.is_empty() {
            return RetentionLeaseFetchResult::default();
        }

        tracing::debug!(
            count = definition_ids.len(),
            "fetching retention leases across definitions"
        );

        fetch_retention_leases_bounded(definition_ids.to_vec(), limit, progress, {
            let client = self.clone();
            move |def_id| {
                let client = client.clone();
                async move { client.list_retention_leases_for_definition(def_id).await }
            }
        })
        .await
    }

    /// Deletes the specified retention leases by their IDs.
    pub async fn delete_retention_leases(&self, ids: &[u32]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        tracing::info!(count = ids.len(), "deleting retention leases");
        let url = self.endpoints.retention_leases_delete(ids);
        self.delete(&url, RequestRetryPolicy::Idempotent).await
    }
}

async fn fetch_retention_leases_bounded<F, Fut>(
    definition_ids: Vec<u32>,
    limit: usize,
    progress: Option<&super::PaginationProgressFn>,
    mut fetch: F,
) -> RetentionLeaseFetchResult
where
    F: FnMut(u32) -> Fut + Send,
    Fut: Future<Output = Result<Vec<RetentionLease>>> + Send + 'static,
{
    let total = definition_ids.len();
    let mut all_leases: HashMap<u32, RetentionLease> = HashMap::new();
    let mut failures = Vec::new();
    let mut completed: usize = 0;

    for_each_bounded(
        definition_ids,
        limit,
        move |def_id| {
            let fut = fetch(def_id);
            async move { (def_id, fut.await) }
        },
        |result| {
            match result {
                Ok((def_id, Ok(leases))) => {
                    for lease in leases {
                        all_leases.entry(lease.lease_id).or_insert(lease);
                    }
                    tracing::trace!(definition_id = def_id, "leases fetched ok");
                }
                Ok((def_id, Err(error))) => {
                    tracing::warn!(definition_id = def_id, error = %error, "failed to fetch leases");
                    failures.push(RetentionLeaseFetchFailure {
                        definition_id: Some(def_id),
                        message: error.to_string(),
                    });
                }
                Err(error) => {
                    tracing::warn!(error = %error, "lease fetch task failed");
                    failures.push(RetentionLeaseFetchFailure {
                        definition_id: None,
                        message: error.to_string(),
                    });
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
        },
    )
    .await;

    if !failures.is_empty() {
        let failed_definition_ids: Vec<u32> =
            failures.iter().filter_map(|f| f.definition_id).collect();
        tracing::warn!(
            failures = failures.len(),
            total,
            failed_definition_ids = ?failed_definition_ids,
            "some lease fetches failed"
        );
    }

    let mut leases: Vec<RetentionLease> = all_leases.into_values().collect();
    leases.sort_by_key(|l| l.lease_id);
    failures.sort_by_key(|failure| failure.definition_id.unwrap_or(u32::MAX));
    RetentionLeaseFetchResult { leases, failures }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use anyhow::anyhow;
    use tokio::sync::{Notify, mpsc};

    use super::*;

    fn lease(id: u32) -> RetentionLease {
        RetentionLease {
            lease_id: id,
            definition_id: id,
            run_id: id * 10,
            owner_id: "test".to_string(),
            created_on: None,
            valid_until: None,
            protect_pipeline: false,
        }
    }

    async fn wait_for_started(rx: &mut mpsc::UnboundedReceiver<()>, expected: usize) {
        for _ in 0..expected {
            tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("task should start before timeout")
                .expect("started sender should remain open");
        }
    }

    async fn wait_until_released(released: Arc<AtomicBool>, release: Arc<Notify>) {
        loop {
            let notified = release.notified();
            if released.load(Ordering::SeqCst) {
                break;
            }
            notified.await;
        }
    }

    #[tokio::test]
    async fn retention_fan_out_never_exceeds_configured_concurrency() {
        let item_count = API_FAN_OUT_LIMIT + 3;
        let active = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let released = Arc::new(AtomicBool::new(false));
        let release = Arc::new(Notify::new());
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();

        let fetch = {
            let active = Arc::clone(&active);
            let max_seen = Arc::clone(&max_seen);
            let completed = Arc::clone(&completed);
            let released = Arc::clone(&released);
            let release = Arc::clone(&release);
            move |definition_id| {
                let active = Arc::clone(&active);
                let max_seen = Arc::clone(&max_seen);
                let completed = Arc::clone(&completed);
                let released = Arc::clone(&released);
                let release = Arc::clone(&release);
                let started_tx = started_tx.clone();
                async move {
                    let active_now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(active_now, Ordering::SeqCst);
                    let _ = started_tx.send(());
                    wait_until_released(released, release).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    completed.fetch_add(1, Ordering::SeqCst);
                    Ok(vec![lease(definition_id)])
                }
            }
        };

        let task = tokio::spawn(fetch_retention_leases_bounded(
            (1..=item_count as u32).collect(),
            API_FAN_OUT_LIMIT,
            None,
            fetch,
        ));

        wait_for_started(&mut started_rx, API_FAN_OUT_LIMIT).await;
        assert_eq!(completed.load(Ordering::SeqCst), 0);
        assert_eq!(max_seen.load(Ordering::SeqCst), API_FAN_OUT_LIMIT);
        assert!(started_rx.try_recv().is_err());

        released.store(true, Ordering::SeqCst);
        release.notify_waiters();

        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("retention fan-out should finish")
            .expect("retention fan-out task should not panic");

        assert_eq!(result.leases.len(), item_count);
        assert!(result.failures.is_empty());
        assert!(max_seen.load(Ordering::SeqCst) <= API_FAN_OUT_LIMIT);
    }

    #[tokio::test]
    async fn retention_fetch_returns_explicit_partial_failures() {
        let result = fetch_retention_leases_bounded(
            vec![1, 2, 3],
            API_FAN_OUT_LIMIT,
            None,
            |id| async move {
                if id == 2 {
                    Err(anyhow!("definition {id} failed"))
                } else {
                    Ok(vec![lease(id)])
                }
            },
        )
        .await;

        let lease_ids: Vec<u32> = result.leases.iter().map(|l| l.lease_id).collect();
        assert_eq!(lease_ids, vec![1, 3]);
        assert!(result.is_partial());
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].definition_id, Some(2));
        assert!(result.failures[0].message.contains("definition 2 failed"));
    }
}
