//! Bounded concurrency helpers for API fan-out workloads.

use std::future::Future;

use tokio::task::{JoinError, JoinSet};

/// Default upper bound for concurrent Azure DevOps fan-out requests.
pub const API_FAN_OUT_LIMIT: usize = 8;

/// Runs asynchronous work for each item with at most `limit` tasks active.
pub async fn for_each_bounded<I, F, Fut, T, H>(items: I, limit: usize, mut task: F, mut handle: H)
where
    I: IntoIterator,
    I::Item: Send + 'static,
    F: FnMut(I::Item) -> Fut,
    Fut: Future<Output = T> + Send + 'static,
    T: Send + 'static,
    H: FnMut(Result<T, JoinError>),
{
    let mut items = items.into_iter();
    let mut set = JoinSet::new();
    let limit = limit.max(1);

    for _ in 0..limit {
        let Some(item) = items.next() else {
            break;
        };
        set.spawn(task(item));
    }

    while let Some(result) = set.join_next().await {
        handle(result);
        if let Some(item) = items.next() {
            set.spawn(task(item));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn for_each_bounded_limits_active_tasks() {
        let active = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));

        for_each_bounded(
            0..10,
            3,
            {
                let active = Arc::clone(&active);
                let max_seen = Arc::clone(&max_seen);
                move |item| {
                    let active = Arc::clone(&active);
                    let max_seen = Arc::clone(&max_seen);
                    async move {
                        let active_now = active.fetch_add(1, Ordering::SeqCst) + 1;
                        max_seen.fetch_max(active_now, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        active.fetch_sub(1, Ordering::SeqCst);
                        item
                    }
                }
            },
            |result| {
                assert!(result.is_ok());
                completed.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;

        assert_eq!(completed.load(Ordering::SeqCst), 10);
        assert!(max_seen.load(Ordering::SeqCst) <= 3);
    }
}
