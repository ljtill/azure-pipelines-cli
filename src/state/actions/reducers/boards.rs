//! Boards reducer logic for backlog, personal work items, and work item details.

use crate::client::models::{BacklogLevelConfiguration, WorkItem, WorkItemComment};
use crate::state::{App, View};

pub(in crate::state::actions) fn boards_loaded(
    app: &mut App,
    team_name: String,
    backlogs: Vec<BacklogLevelConfiguration>,
    work_items: Vec<WorkItem>,
    partial_errors: &[String],
    generation: u64,
) {
    if generation != app.boards.generation {
        tracing::debug!(
            generation,
            expected = app.boards.generation,
            "dropping obsolete boards response"
        );
        return;
    }
    tracing::info!(
        team = %team_name,
        backlogs = backlogs.len(),
        work_items = work_items.len(),
        partial_errors = partial_errors.len(),
        "boards loaded"
    );
    let query = app.search.query.clone();
    app.boards
        .set_data_with_errors(team_name, backlogs, work_items, &query, partial_errors);
}

pub(in crate::state::actions) fn boards_failed(app: &mut App, message: String, generation: u64) {
    if generation != app.boards.generation {
        tracing::debug!(
            generation,
            expected = app.boards.generation,
            "dropping obsolete boards failure"
        );
        return;
    }
    tracing::warn!(%message, "boards fetch failed");
    app.boards.set_error(message.clone());
    app.notifications.error_dedup(message);
}

pub(in crate::state::actions) fn my_work_items_loaded(
    app: &mut App,
    view: View,
    work_items: &[WorkItem],
    generation: u64,
) {
    let query = app.search.query.clone();
    let Some(list) = app.my_work_items.list_for_mut(view) else {
        return;
    };
    if generation != list.generation {
        tracing::debug!(
            ?view,
            generation,
            expected = list.generation,
            "dropping obsolete my work items response"
        );
        return;
    }
    tracing::info!(?view, count = work_items.len(), "my work items loaded");
    list.set_data(work_items, &query);
}

pub(in crate::state::actions) fn my_work_items_failed(
    app: &mut App,
    view: View,
    message: String,
    generation: u64,
) {
    let Some(list) = app.my_work_items.list_for_mut(view) else {
        return;
    };
    if generation != list.generation {
        tracing::debug!(
            ?view,
            generation,
            expected = list.generation,
            "dropping obsolete my work items failure"
        );
        return;
    }
    tracing::warn!(?view, %message, "my work items fetch failed");
    app.notifications.error_dedup(message);
}

pub(in crate::state::actions) fn work_item_detail_loaded(
    app: &mut App,
    work_item_id: u32,
    work_item: Box<WorkItem>,
    comments: Vec<WorkItemComment>,
) {
    if app.work_item_detail.work_item_id == Some(work_item_id) {
        tracing::info!(
            work_item_id,
            comments = comments.len(),
            "work item detail loaded"
        );
        app.work_item_detail.work_item = Some(*work_item);
        app.work_item_detail.comments = comments;
        app.work_item_detail.loading = false;
        let section_count = app.work_item_detail.section_count();
        app.work_item_detail.nav.set_len(section_count);
    } else {
        tracing::debug!(
            work_item_id,
            pending = ?app.work_item_detail.work_item_id,
            "ignoring stale work item detail response"
        );
    }
}

pub(in crate::state::actions) fn work_item_detail_failed(
    app: &mut App,
    work_item_id: u32,
    message: String,
) {
    if app.work_item_detail.work_item_id == Some(work_item_id) {
        tracing::warn!(work_item_id, %message, "work item detail fetch failed");
        app.work_item_detail.loading = false;
    }
    app.notifications.error(message);
}
