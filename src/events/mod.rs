//! Keyboard and mouse event dispatch to per-view handlers.

mod active_runs;
mod build_history;
mod common;
mod dashboard;
mod log_viewer;
mod navigation;
mod pipelines;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use crate::state::{App, ConfirmAction, ConfirmPrompt, InputMode, View};

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
        KeyCode::Char('x')
            if app.view == View::Dashboard
                || app.view == View::Pipelines
                || app.view == View::ActiveRuns =>
        {
            app.notifications.clear();
            Some(Action::None)
        }

        // Tab switching.
        KeyCode::Char('1') => {
            tracing::info!(from = ?app.view, to = ?View::Dashboard, "view switch");
            app.search.query.clear();
            app.view = View::Dashboard;
            Some(Action::None)
        }
        KeyCode::Char('2') => {
            tracing::info!(from = ?app.view, to = ?View::Pipelines, "view switch");
            app.search.query.clear();
            app.view = View::Pipelines;
            app.pipelines.rebuild(
                &app.data.definitions,
                &app.filters.folders,
                &app.filters.definition_ids,
                &app.search.query,
            );
            Some(Action::None)
        }
        KeyCode::Char('3') => {
            tracing::info!(from = ?app.view, to = ?View::ActiveRuns, "view switch");
            app.search.query.clear();
            app.view = View::ActiveRuns;
            app.active_runs.rebuild(
                &app.data.active_builds,
                &app.filters.definition_ids,
                &app.search.query,
            );
            Some(Action::None)
        }

        // Navigation.
        KeyCode::Up => {
            app.current_nav_mut().up();
            Some(Action::None)
        }
        KeyCode::Down if app.view != View::BuildHistory => {
            app.current_nav_mut().down();
            Some(Action::None)
        }
        KeyCode::Home => {
            app.current_nav_mut().home();
            Some(Action::None)
        }
        KeyCode::End if app.view != View::BuildHistory => {
            app.current_nav_mut().end();
            Some(Action::None)
        }

        // q/Esc — go back logic.
        KeyCode::Char('q') => Some(handle_quit_key(app)),
        KeyCode::Esc => Some(handle_esc_key(app)),

        _ => None, // Fall through to view-specific.
    }
}

/// Handles the quit key by navigating back or prompting to confirm quit.
fn handle_quit_key(app: &mut App) -> Action {
    match app.view {
        View::Dashboard => {
            app.confirm_prompt = Some(ConfirmPrompt {
                message: "Quit? (y/n)".into(),
                action: ConfirmAction::Quit,
            });
            Action::None
        }
        View::Pipelines | View::ActiveRuns => {
            app.search.mode = InputMode::Normal;
            app.search.query.clear();
            app.view = View::Dashboard;
            Action::None
        }
        _ => {
            app.go_back();
            Action::None
        }
    }
}

/// Handles the Esc key by navigating back to the parent view.
fn handle_esc_key(app: &mut App) -> Action {
    match app.view {
        View::Dashboard => Action::None,
        View::Pipelines | View::ActiveRuns => {
            app.search.mode = InputMode::Normal;
            app.search.query.clear();
            app.view = View::Dashboard;
            Action::None
        }
        _ => {
            app.go_back();
            Action::None
        }
    }
}

/// Handles mouse scroll events in the log viewer.
pub fn handle_mouse(app: &mut App, mouse: MouseEvent) -> Action {
    match mouse.kind {
        MouseEventKind::ScrollUp if app.view == View::LogViewer => {
            if let Some(tree_area) = app.log_viewer.tree_area() {
                if mouse.column >= tree_area.x
                    && mouse.column < tree_area.x + tree_area.width
                    && mouse.row >= tree_area.y
                    && mouse.row < tree_area.y + tree_area.height
                {
                    for _ in 0..3 {
                        app.log_viewer.nav_mut().up();
                    }
                    return Action::None;
                }
            }
            app.log_viewer.scroll_up(3);
            Action::None
        }
        MouseEventKind::ScrollDown if app.view == View::LogViewer => {
            if let Some(tree_area) = app.log_viewer.tree_area() {
                if mouse.column >= tree_area.x
                    && mouse.column < tree_area.x + tree_area.width
                    && mouse.row >= tree_area.y
                    && mouse.row < tree_area.y + tree_area.height
                {
                    for _ in 0..3 {
                        app.log_viewer.nav_mut().down();
                    }
                    return Action::None;
                }
            }
            app.log_viewer.scroll_down(3);
            Action::None
        }
        _ => Action::None,
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

    // --- Esc on top-level peer views ---

    #[test]
    fn esc_on_pipelines_goes_to_dashboard() {
        let mut app = make_app();
        app.view = View::Pipelines;

        handle_key(&mut app, key(KeyCode::Esc));
        assert_eq!(app.view, View::Dashboard);
    }

    #[test]
    fn esc_on_active_runs_goes_to_dashboard() {
        let mut app = make_app();
        app.view = View::ActiveRuns;

        handle_key(&mut app, key(KeyCode::Esc));
        assert_eq!(app.view, View::Dashboard);
    }

    #[test]
    fn esc_on_dashboard_is_noop() {
        let mut app = make_app();
        app.view = View::Dashboard;

        let action = handle_key(&mut app, key(KeyCode::Esc));
        assert_eq!(app.view, View::Dashboard);
        assert!(matches!(action, Action::None));
    }
}
