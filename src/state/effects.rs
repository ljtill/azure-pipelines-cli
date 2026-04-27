//! Tracks owned background effects and their cancellation handles.

use std::collections::BTreeMap;

use tokio::task::JoinHandle;

use super::View;

/// Represents a cancellable background task category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EffectKind {
    DashboardPullRequests,
    DashboardWorkItems,
    PinnedWorkItems,
    BuildHistoryRefresh,
    TimelineFetch,
    LogFetch,
    LogRefresh,
    PullRequestsRefresh,
    PullRequestDetail,
    BoardsRefresh,
    MyWorkItemsAssigned,
    MyWorkItemsCreated,
    WorkItemDetail,
}

impl EffectKind {
    /// Lists every effect kind whose lifecycle is scoped to the active view.
    pub const VIEW_SCOPED: [Self; 13] = [
        Self::DashboardPullRequests,
        Self::DashboardWorkItems,
        Self::PinnedWorkItems,
        Self::BuildHistoryRefresh,
        Self::TimelineFetch,
        Self::LogFetch,
        Self::LogRefresh,
        Self::PullRequestsRefresh,
        Self::PullRequestDetail,
        Self::BoardsRefresh,
        Self::MyWorkItemsAssigned,
        Self::MyWorkItemsCreated,
        Self::WorkItemDetail,
    ];

    /// Returns the effect kind for a personal Boards view.
    pub fn my_work_items(view: View) -> Option<Self> {
        match view {
            View::BoardsAssignedToMe => Some(Self::MyWorkItemsAssigned),
            View::BoardsCreatedByMe => Some(Self::MyWorkItemsCreated),
            _ => None,
        }
    }

    /// Returns whether this effect kind should remain active for `view`.
    pub fn is_active_for_view(self, view: View) -> bool {
        match self {
            Self::DashboardPullRequests | Self::DashboardWorkItems | Self::PinnedWorkItems => {
                view == View::Dashboard
            }
            Self::BuildHistoryRefresh => view == View::BuildHistory,
            Self::TimelineFetch | Self::LogFetch | Self::LogRefresh => view == View::LogViewer,
            Self::PullRequestsRefresh => view.is_pull_requests(),
            Self::PullRequestDetail => view == View::PullRequestDetail,
            Self::BoardsRefresh => view == View::Boards,
            Self::MyWorkItemsAssigned => view == View::BoardsAssignedToMe,
            Self::MyWorkItemsCreated => view == View::BoardsCreatedByMe,
            Self::WorkItemDetail => view == View::WorkItemDetail,
        }
    }
}

/// Describes the lifecycle metadata for an owned background effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectMetadata {
    pub kind: EffectKind,
    pub generation: Option<u64>,
}

impl EffectMetadata {
    /// Returns metadata for a tracked effect.
    #[must_use]
    pub fn new(kind: EffectKind, generation: Option<u64>) -> Self {
        Self { kind, generation }
    }
}

#[derive(Debug)]
struct ManagedEffect {
    metadata: EffectMetadata,
    handle: JoinHandle<()>,
}

impl ManagedEffect {
    fn abort(self) -> EffectMetadata {
        if !self.handle.is_finished() {
            self.handle.abort();
        }
        self.metadata
    }
}

/// Owns cancellable background task handles keyed by effect kind.
#[derive(Debug, Default)]
pub struct EffectManager {
    tasks: BTreeMap<EffectKind, ManagedEffect>,
}

impl EffectManager {
    /// Tracks a task and aborts any older task with the same effect kind.
    pub fn track(
        &mut self,
        kind: EffectKind,
        generation: Option<u64>,
        handle: JoinHandle<()>,
    ) -> Option<EffectMetadata> {
        self.prune_finished();
        let metadata = EffectMetadata::new(kind, generation);
        self.tasks
            .insert(kind, ManagedEffect { metadata, handle })
            .map(ManagedEffect::abort)
    }

    /// Cancels a tracked task by effect kind.
    pub fn cancel(&mut self, kind: EffectKind) -> Option<EffectMetadata> {
        self.tasks.remove(&kind).map(ManagedEffect::abort)
    }

    /// Cancels every tracked task.
    pub fn cancel_all(&mut self) {
        for (_, task) in std::mem::take(&mut self.tasks) {
            task.abort();
        }
    }

    /// Removes finished task handles from the manager.
    pub fn prune_finished(&mut self) {
        self.tasks.retain(|_, task| !task.handle.is_finished());
    }

    /// Returns the generation carried by the active task for an effect kind.
    pub fn generation(&self, kind: EffectKind) -> Option<u64> {
        self.tasks
            .get(&kind)
            .and_then(|task| task.metadata.generation)
    }

    /// Returns whether a task for the effect kind is still running.
    pub fn is_running(&self, kind: EffectKind) -> bool {
        self.tasks
            .get(&kind)
            .is_some_and(|task| !task.handle.is_finished())
    }

    /// Returns the number of tracked task handles.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Returns whether no task handles are currently tracked.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

impl Drop for EffectManager {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::time::Duration;

    use tokio::sync::oneshot;

    use super::*;

    struct DropSignal(Option<oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }

    fn pending_task(tx: oneshot::Sender<()>) -> JoinHandle<()> {
        let signal = DropSignal(Some(tx));
        tokio::spawn(async move {
            let _signal = signal;
            future::pending::<()>().await;
        })
    }

    #[tokio::test]
    async fn track_replaces_existing_effect_and_aborts_old_handle() {
        let mut manager = EffectManager::default();
        let (first_tx, first_rx) = oneshot::channel();
        let (second_tx, _second_rx) = oneshot::channel();

        assert!(
            manager
                .track(
                    EffectKind::BuildHistoryRefresh,
                    Some(1),
                    pending_task(first_tx),
                )
                .is_none()
        );
        assert_eq!(manager.generation(EffectKind::BuildHistoryRefresh), Some(1));

        let replaced = manager
            .track(
                EffectKind::BuildHistoryRefresh,
                Some(2),
                pending_task(second_tx),
            )
            .expect("existing effect should be replaced");

        assert_eq!(replaced.kind, EffectKind::BuildHistoryRefresh);
        assert_eq!(replaced.generation, Some(1));
        tokio::time::timeout(Duration::from_secs(1), first_rx)
            .await
            .expect("old task was not aborted")
            .expect("old task drop signal was not delivered");
        assert_eq!(manager.generation(EffectKind::BuildHistoryRefresh), Some(2));
    }

    #[tokio::test]
    async fn prune_finished_removes_completed_handles() {
        let mut manager = EffectManager::default();
        let _ = manager.track(
            EffectKind::PullRequestsRefresh,
            Some(7),
            tokio::spawn(async {}),
        );
        assert_eq!(manager.len(), 1);

        tokio::time::timeout(Duration::from_secs(1), async {
            while manager.is_running(EffectKind::PullRequestsRefresh) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("task did not finish");

        manager.prune_finished();
        assert!(manager.is_empty());
    }

    #[tokio::test]
    async fn drop_aborts_owned_effects() {
        let (tx, rx) = oneshot::channel();
        {
            let mut manager = EffectManager::default();
            let _ = manager.track(EffectKind::BoardsRefresh, None, pending_task(tx));
        }

        tokio::time::timeout(Duration::from_secs(1), rx)
            .await
            .expect("owned task was not aborted on drop")
            .expect("drop signal was not delivered");
    }

    #[test]
    fn my_work_items_kind_maps_personal_boards_views() {
        assert_eq!(
            EffectKind::my_work_items(View::BoardsAssignedToMe),
            Some(EffectKind::MyWorkItemsAssigned)
        );
        assert_eq!(
            EffectKind::my_work_items(View::BoardsCreatedByMe),
            Some(EffectKind::MyWorkItemsCreated)
        );
        assert_eq!(EffectKind::my_work_items(View::Boards), None);
    }
}
