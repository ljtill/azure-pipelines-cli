//! Event handling for the active runs view.

use crossterm::event::{KeyCode, KeyEvent};

use super::Action;
use super::navigation;
use crate::state::{App, InputMode};

/// Handles key events specific to the active runs view.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('/') => {
            tracing::debug!(view = ?app.view, "entering search mode");
            app.search.mode = InputMode::Search;
            Action::None
        }
        // Multi-select toggle.
        KeyCode::Char(' ') => {
            app.shell
                .views
                .active_runs
                .toggle_selected_at_cursor(&app.core.data.active_builds);
            Action::None
        }
        KeyCode::Char('c') => navigation::handle_cancel_request(app),
        KeyCode::Right | KeyCode::Enter => handle_enter_active_runs(app),
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}

/// Handles the Enter key on active runs, drilling into the log viewer.
fn handle_enter_active_runs(app: &mut App) -> Action {
    let index = app.active_runs.nav.index();
    if let Some(build) = app
        .shell
        .views
        .active_runs
        .build_at(&app.core.data.active_builds, index)
        .cloned()
    {
        let build_id = build.id;
        app.navigate_to_log_viewer(build);
        Action::FetchTimeline(build_id)
    } else {
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use crate::client::models::*;
    use crate::events::{Action, handle_key};
    use crate::test_helpers::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use crate::state::View;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn right_arrow_on_active_runs_drills_into_log_viewer() {
        let mut app = make_app();
        app.view = View::ActiveRuns;
        let build = make_build(200, BuildStatus::InProgress, None);
        app.core.data.active_builds = vec![build];
        let query = app.search.query.clone();
        app.shell.views.active_runs.rebuild(
            &app.core.data.active_builds,
            &app.core.filters.definition_ids,
            &query,
        );

        let action = handle_key(&mut app, key(KeyCode::Right));
        assert_eq!(app.view, View::LogViewer);
        assert!(matches!(action, Action::FetchTimeline(_)));
    }

    #[test]
    fn left_arrow_on_active_runs_is_noop() {
        let mut app = make_app();
        app.view = View::ActiveRuns;

        let action = handle_key(&mut app, key(KeyCode::Left));
        assert_eq!(app.view, View::ActiveRuns);
        assert!(matches!(action, Action::None));
    }
}
