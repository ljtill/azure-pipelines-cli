//! Shared helpers for toggling pinned items from various views.

use super::Action;
use crate::state::App;

/// Toggles pin state for a work item ID; persists config and triggers a refetch.
///
/// Returns `Action::FetchPinnedWorkItems` so the dashboard's pinned section stays in sync.
pub fn toggle_work_item_pin(app: &mut App, id: u32) -> Action {
    let was_pinned = app.filters.pinned_work_item_ids.contains(&id);
    if was_pinned {
        app.filters.pinned_work_item_ids.retain(|pid| *pid != id);
        app.notifications
            .success(format!("Unpinned work item #{id}"));
    } else {
        app.filters.pinned_work_item_ids.push(id);
        app.notifications.success(format!("Pinned work item #{id}"));
    }

    let config = app.current_config();
    if let Err(e) = config.save(&app.config_path) {
        tracing::error!(%e, "failed to save config after work item pin toggle");
        app.notifications
            .error(format!("Failed to save config: {e}"));
    }

    Action::FetchPinnedWorkItems
}
