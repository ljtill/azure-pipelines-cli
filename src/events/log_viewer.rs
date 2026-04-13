//! Event handling for the log viewer.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;

use super::Action;
use super::navigation;
use crate::state::{App, TimelineRow};

/// Handles key events specific to the log viewer.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('f') => {
            app.log_viewer.enter_follow_mode();
            Action::FollowLatest
        }
        KeyCode::Left => {
            let idx = app.log_viewer.nav().index();
            match app.log_viewer.timeline_row_kind(idx) {
                Some("stage") => {
                    app.log_viewer.collapse_timeline_node(idx);
                }
                Some("job") => {
                    if !app.log_viewer.collapse_timeline_node(idx)
                        && let Some(parent_idx) = app.log_viewer.find_timeline_parent_index(idx)
                    {
                        app.log_viewer.nav_mut().set_index(parent_idx);
                    }
                }
                Some("task") => {
                    if let Some(parent_idx) = app.log_viewer.find_timeline_parent_index(idx) {
                        app.log_viewer.nav_mut().set_index(parent_idx);
                    }
                }
                _ => {}
            }
            Action::None
        }
        KeyCode::Right => {
            let idx = app.log_viewer.nav().index();
            match app.log_viewer.timeline_row_kind(idx) {
                Some("stage" | "job") => {
                    app.log_viewer.expand_timeline_node(idx);
                }
                Some("task") => {
                    return handle_enter_log_viewer(app);
                }
                _ => {}
            }
            Action::None
        }
        KeyCode::Enter => handle_enter_log_viewer(app),
        KeyCode::PageUp => {
            app.log_viewer.scroll_up(20);
            Action::None
        }
        KeyCode::PageDown => {
            app.log_viewer.scroll_down(20);
            Action::None
        }
        KeyCode::Char('c') => navigation::handle_cancel_request(app),
        KeyCode::Char('R') => handle_retry_request(app),
        KeyCode::Char('A') => handle_approve_request(app),
        KeyCode::Char('D') => handle_reject_request(app),
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}

/// Handles the Enter key on the log viewer, toggling nodes or fetching task logs.
fn handle_enter_log_viewer(app: &mut App) -> Action {
    let idx = app.log_viewer.nav().index();
    match app.log_viewer.timeline_row_kind(idx) {
        Some("stage" | "job") => {
            app.log_viewer.toggle_timeline_node(idx);
            Action::None
        }
        Some("task") => {
            app.log_viewer.enter_inspect_mode();
            if let Some(log_id) = app.log_viewer.timeline_task_log_id(idx)
                && let Some(build) = app.log_viewer.selected_build()
            {
                return Action::FetchBuildLog {
                    build_id: build.id,
                    log_id,
                };
            }
            Action::None
        }
        _ => Action::None,
    }
}

/// Prompts the user to confirm retrying the stage under the cursor.
fn handle_retry_request(app: &mut App) -> Action {
    let idx = app.log_viewer.nav().index();

    // Find the stage to retry: if cursor is on a stage, use it directly.
    // If on a job/task/checkpoint, walk up to the parent stage.
    let stage_idx = match app.log_viewer.timeline_row_kind(idx) {
        Some("stage") => Some(idx),
        Some("job") => app.log_viewer.find_timeline_parent_index(idx),
        Some("task") => app
            .log_viewer
            .find_timeline_parent_index(idx)
            .and_then(|job_idx| app.log_viewer.find_timeline_parent_index(job_idx)),
        Some("checkpoint") => {
            if let Some(TimelineRow::Checkpoint {
                parent_stage_id, ..
            }) = app.log_viewer.timeline_rows().get(idx)
            {
                let psid = parent_stage_id.clone();
                app.log_viewer
                    .timeline_rows()
                    .iter()
                    .position(|r| matches!(r, TimelineRow::Stage { id, .. } if *id == psid))
            } else {
                None
            }
        }
        _ => None,
    };

    let Some(stage_idx) = stage_idx else {
        return Action::None;
    };

    let Some(stage_ref_name) = app.log_viewer.timeline_stage_ref_name(stage_idx) else {
        return Action::None;
    };
    let build_id = match app.log_viewer.selected_build() {
        Some(b) => b.id,
        None => return Action::None,
    };
    let build_number = app
        .log_viewer
        .selected_build()
        .map_or("?", |b| b.build_number.as_str());
    let stage_name = match app.log_viewer.timeline_rows().get(stage_idx) {
        Some(TimelineRow::Stage { name, .. }) => name.clone(),
        _ => stage_ref_name.clone(),
    };

    app.confirm_prompt = Some(crate::state::ConfirmPrompt {
        message: format!("Retry stage \"{stage_name}\" in build #{build_number}?  [y/N]"),
        action: crate::state::ConfirmAction::RetryStage {
            build_id,
            stage_ref_name,
        },
    });
    Action::None
}

/// Prompts the user to approve the checkpoint under the cursor.
fn handle_approve_request(app: &mut App) -> Action {
    let idx = app.log_viewer.nav().index();
    if app.log_viewer.timeline_row_kind(idx) != Some("checkpoint") {
        return Action::None;
    }
    let Some(approval_id) = app.log_viewer.timeline_approval_id(idx) else {
        return Action::None;
    };
    let name = match app.log_viewer.timeline_rows().get(idx) {
        Some(crate::state::TimelineRow::Checkpoint { name, .. }) => name.clone(),
        _ => "check".to_string(),
    };
    app.confirm_prompt = Some(crate::state::ConfirmPrompt {
        message: format!("Approve \"{name}\"?  [y/N]"),
        action: crate::state::ConfirmAction::ApproveCheck { approval_id },
    });
    Action::None
}

/// Prompts the user to reject the checkpoint under the cursor.
fn handle_reject_request(app: &mut App) -> Action {
    let idx = app.log_viewer.nav().index();
    if app.log_viewer.timeline_row_kind(idx) != Some("checkpoint") {
        return Action::None;
    }
    let Some(approval_id) = app.log_viewer.timeline_approval_id(idx) else {
        return Action::None;
    };
    let name = match app.log_viewer.timeline_rows().get(idx) {
        Some(crate::state::TimelineRow::Checkpoint { name, .. }) => name.clone(),
        _ => "check".to_string(),
    };
    app.confirm_prompt = Some(crate::state::ConfirmPrompt {
        message: format!("Reject \"{name}\"?  [y/N]"),
        action: crate::state::ConfirmAction::RejectCheck { approval_id },
    });
    Action::None
}
