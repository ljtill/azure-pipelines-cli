//! Keyboard and mouse event dispatch to per-view handlers.

mod active_runs;
mod boards;
mod build_history;
mod common;
mod dashboard;
mod log_viewer;
mod my_work_items;
mod navigation;
mod pins;
mod pipelines;
mod pull_request_detail;
mod pull_requests;
mod work_item_detail;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use crate::state::{App, ConfirmAction, ConfirmPrompt, InputMode, Service, View};

/// Represents the action requested by the user after handling a key event.
#[derive(Debug)]
pub enum Action {
    None,
    Quit,
    ForceRefresh,
    FetchBuildHistory(u32),
    FetchMoreBuilds {
        definition_id: u32,
        continuation_token: String,
    },
    FetchBuildLog {
        build_id: u32,
        log_id: u32,
    },
    FetchTimeline(u32),
    FollowLatest,
    OpenInBrowser(String),
    CancelBuild(u32),
    CancelBuilds(Vec<u32>),
    RetryStage {
        build_id: u32,
        stage_ref_name: String,
    },
    QueuePipeline(u32),
    ApproveCheck(String),
    RejectCheck(String),
    Reload,
    DeleteRetentionLeases(Vec<u32>),
    FetchPullRequests,
    FetchPullRequestDetail {
        repo_id: String,
        pr_id: u32,
    },
    FetchWorkItemDetail {
        work_item_id: u32,
    },
    FetchDashboardPullRequests,
    FetchDashboardWorkItems,
    FetchPinnedWorkItems,
    FetchBoards,
    FetchMyWorkItems,
}

/// Dispatches a key event to the appropriate view-specific handler.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    tracing::trace!(key = ?key.code, modifiers = ?key.modifiers, view = ?app.view, "key event");

    // Ctrl+C always quits.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Action::Quit;
    }

    // Confirmation prompt — only accept y/n/Esc.
    if app.confirm_prompt.is_some() {
        return common::handle_confirm_key(app, key);
    }

    // Search mode input.
    if app.search.mode == InputMode::Search {
        return common::handle_search_key(app, key);
    }

    // Help overlay — any key dismisses.
    if app.show_help {
        app.show_help = false;
        return Action::None;
    }

    // Settings overlay — route keys to dedicated handler.
    if app.show_settings {
        return common::handle_settings_key(app, key);
    }

    // Common keys (work in all views).
    if let Some(action) = handle_common_key(app, key) {
        return action;
    }

    // View-specific keys.
    match app.view {
        View::Dashboard => dashboard::handle_key(app, key),
        View::Pipelines => pipelines::handle_key(app, key),
        View::ActiveRuns => active_runs::handle_key(app, key),
        View::BuildHistory => build_history::handle_key(app, key),
        View::LogViewer => log_viewer::handle_key(app, key),
        View::PullRequestsCreatedByMe
        | View::PullRequestsAssignedToMe
        | View::PullRequestsAllActive => pull_requests::handle_key(app, key),
        View::PullRequestDetail => pull_request_detail::handle_key(app, key),
        View::Boards => boards::handle_key(app, key),
        View::BoardsAssignedToMe | View::BoardsCreatedByMe => my_work_items::handle_key(app, key),
        View::WorkItemDetail => work_item_detail::handle_key(app, key),
    }
}

/// Handles keys that work identically across all views.
fn handle_common_key(app: &mut App, key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('?') => {
            tracing::debug!("opening help overlay");
            app.show_help = true;
            Some(Action::None)
        }
        KeyCode::Char(',') => {
            tracing::debug!("opening settings");
            app.open_settings();
            Some(Action::None)
        }
        KeyCode::Char('r') => Some(Action::ForceRefresh),
        KeyCode::Char('x') if app.view.is_root() => {
            app.notifications.clear();
            Some(Action::None)
        }

        // Top-level area switching.
        KeyCode::Char('1') => Some(activate_service(app, Service::Dashboard)),
        KeyCode::Char('2') => Some(activate_service(app, Service::Boards)),
        KeyCode::Char('3') => Some(activate_service(app, Service::Repos)),
        KeyCode::Char('4') => Some(activate_service(app, Service::Pipelines)),

        // Sub-view cycling within the current service.
        KeyCode::Tab if app.view.is_root() => Some(cycle_root_view(app, 1)),
        KeyCode::BackTab if app.view.is_root() => Some(cycle_root_view(app, -1)),

        // Navigation.
        KeyCode::Up => {
            app.current_nav_mut().up();
            if app.view == View::Dashboard {
                app.dashboard.skip_separator(false);
            }
            Some(Action::None)
        }
        KeyCode::Down if app.view != View::BuildHistory => {
            app.current_nav_mut().down();
            if app.view == View::Dashboard {
                app.dashboard.skip_separator(true);
            }
            Some(Action::None)
        }
        KeyCode::Home => {
            app.current_nav_mut().home();
            if app.view == View::Dashboard {
                app.dashboard.skip_separator(true);
            }
            Some(Action::None)
        }
        KeyCode::End if app.view != View::BuildHistory => {
            app.current_nav_mut().end();
            if app.view == View::Dashboard {
                app.dashboard.skip_separator(false);
            }
            Some(Action::None)
        }

        // q/Esc — go back logic.
        KeyCode::Char('q') => Some(handle_quit_key(app)),
        KeyCode::Esc => Some(handle_esc_key(app)),

        _ => None, // Fall through to view-specific.
    }
}

/// Handles the quit key by navigating back or jumping to Dashboard.
fn handle_quit_key(app: &mut App) -> Action {
    match app.view {
        View::Dashboard => {
            app.confirm_prompt = Some(ConfirmPrompt {
                message: "Quit? (y/n)".into(),
                action: ConfirmAction::Quit,
            });
            Action::None
        }
        view if view.is_root() => {
            app.activate_root_view(View::Dashboard);
            Action::None
        }
        _ => {
            app.go_back();
            Action::None
        }
    }
}

/// Handles the Esc key by navigating up one level.
fn handle_esc_key(app: &mut App) -> Action {
    if app.view.is_root() {
        // No-op at the top level — Esc always means "up one level".
        Action::None
    } else {
        app.go_back();
        Action::None
    }
}

/// Handles mouse scroll events in the log viewer.
pub fn handle_mouse(app: &mut App, mouse: MouseEvent) -> Action {
    match mouse.kind {
        MouseEventKind::ScrollUp if app.view == View::LogViewer => {
            if let Some(tree_area) = app.log_viewer.tree_area()
                && mouse.column >= tree_area.x
                && mouse.column < tree_area.x + tree_area.width
                && mouse.row >= tree_area.y
                && mouse.row < tree_area.y + tree_area.height
            {
                for _ in 0..3 {
                    app.log_viewer.nav_mut().up();
                }
                return Action::None;
            }
            app.log_viewer.scroll_up(3);
            Action::None
        }
        MouseEventKind::ScrollDown if app.view == View::LogViewer => {
            if let Some(tree_area) = app.log_viewer.tree_area()
                && mouse.column >= tree_area.x
                && mouse.column < tree_area.x + tree_area.width
                && mouse.row >= tree_area.y
                && mouse.row < tree_area.y + tree_area.height
            {
                for _ in 0..3 {
                    app.log_viewer.nav_mut().down();
                }
                return Action::None;
            }
            app.log_viewer.scroll_down(3);
            Action::None
        }
        _ => Action::None,
    }
}

fn activate_service(app: &mut App, service: Service) -> Action {
    let target = app.select_service(service);
    action_for_root_view(target)
}

/// Cycles to the next/prev sub-view within the currently active service and
/// returns the appropriate `Action` (e.g. trigger a fetch if the new view requires data).
fn cycle_root_view(app: &mut App, delta: i32) -> Action {
    // Services with only one root view have nothing to cycle to.
    if app.service.root_views().len() <= 1 {
        return Action::None;
    }
    let target = app.cycle_root_view(delta);
    action_for_root_view(target)
}

fn action_for_root_view(view: View) -> Action {
    match view {
        View::Dashboard => Action::FetchDashboardPullRequests,
        View::Boards => Action::FetchBoards,
        View::BoardsAssignedToMe | View::BoardsCreatedByMe => Action::FetchMyWorkItems,
        View::PullRequestsCreatedByMe
        | View::PullRequestsAssignedToMe
        | View::PullRequestsAllActive => Action::FetchPullRequests,
        View::ActiveRuns
        | View::Pipelines
        | View::BuildHistory
        | View::LogViewer
        | View::PullRequestDetail
        | View::WorkItemDetail => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::models::*;
    use crate::test_helpers::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // --- Esc on drill-in views goes back ---

    #[test]
    fn esc_on_build_history_goes_back() {
        let mut app = make_app();
        app.view = View::Dashboard;
        let def = make_definition(1, "P", "\\");
        app.navigate_to_build_history(def);
        assert_eq!(app.view, View::BuildHistory);

        handle_key(&mut app, key(KeyCode::Esc));
        assert_eq!(app.view, View::Dashboard);
    }

    #[test]
    fn esc_on_log_viewer_goes_back() {
        let mut app = make_app();
        let def = make_definition(1, "P", "\\");
        app.navigate_to_build_history(def);
        let build = make_build(400, BuildStatus::Completed, Some(BuildResult::Succeeded));
        app.navigate_to_log_viewer(build);
        assert_eq!(app.view, View::LogViewer);

        handle_key(&mut app, key(KeyCode::Esc));
        assert_eq!(app.view, View::BuildHistory);
    }

    // --- Esc on top-level root views is no-op ---

    #[test]
    fn esc_on_pipelines_is_noop() {
        let mut app = make_app();
        app.view = View::Pipelines;
        app.service = Service::Pipelines;

        let action = handle_key(&mut app, key(KeyCode::Esc));
        assert_eq!(app.view, View::Pipelines);
        assert!(matches!(action, Action::None));
    }

    #[test]
    fn esc_on_active_runs_is_noop() {
        let mut app = make_app();
        app.view = View::ActiveRuns;
        app.service = Service::Pipelines;

        let action = handle_key(&mut app, key(KeyCode::Esc));
        assert_eq!(app.view, View::ActiveRuns);
        assert!(matches!(action, Action::None));
    }

    #[test]
    fn esc_on_dashboard_is_noop() {
        let mut app = make_app();
        app.view = View::Dashboard;

        let action = handle_key(&mut app, key(KeyCode::Esc));
        assert_eq!(app.view, View::Dashboard);
        assert!(matches!(action, Action::None));
    }

    // --- Mouse scroll with stale cached rects (post-resize) ---

    fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn scroll_outside_stale_tree_area_falls_through_to_log_scroll() {
        use ratatui::layout::Rect;

        // Set up a LogViewer with cached layout rects that no longer match
        // the current viewport — e.g. after the terminal was resized smaller
        // between the last render and this mouse event.
        let mut app = make_app();
        app.view = View::LogViewer;
        app.log_viewer
            .set_layout_areas(Rect::new(0, 0, 30, 10), Rect::new(30, 0, 50, 10));

        // Scroll event far outside the cached tree_area (as if the terminal
        // shrank and the column is beyond anything we know about).
        let ev = mouse(MouseEventKind::ScrollDown, 200, 200);
        let action = handle_mouse(&mut app, ev);
        assert!(matches!(action, Action::None));
        // Falls through to log_scroll, which accumulates as a no-op-safe
        // offset (no panic, no out-of-bounds access).
        assert_eq!(app.log_viewer.log_scroll_offset(), 3);

        let ev = mouse(MouseEventKind::ScrollUp, 200, 200);
        let action = handle_mouse(&mut app, ev);
        assert!(matches!(action, Action::None));
        // scroll_up decreases by 3, back to 0.
        assert_eq!(app.log_viewer.log_scroll_offset(), 0);
    }

    #[test]
    fn scroll_inside_tree_area_drives_nav_not_log_scroll() {
        use ratatui::layout::Rect;

        let mut app = make_app();
        app.view = View::LogViewer;
        app.log_viewer
            .set_layout_areas(Rect::new(0, 0, 30, 10), Rect::new(30, 0, 50, 10));

        // Point inside tree_area.
        let ev = mouse(MouseEventKind::ScrollDown, 5, 5);
        let action = handle_mouse(&mut app, ev);
        assert!(matches!(action, Action::None));
        // Log scroll offset should NOT have advanced when the hit was inside
        // the tree pane.
        assert_eq!(app.log_viewer.log_scroll_offset(), 0);
    }
}
