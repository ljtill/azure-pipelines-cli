//! Event handling for the build history view.

use crossterm::event::{KeyCode, KeyEvent};

use super::Action;
use super::navigation;
use crate::state::App;

/// Handles key events specific to the build history view.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        // Multi-select toggle for lease deletion.
        KeyCode::Char(' ') => {
            app.build_history.toggle_selected_at_cursor();
            Action::None
        }
        KeyCode::Char('c') => navigation::handle_cancel_request(app),
        KeyCode::Char('d') => navigation::handle_delete_build_leases_request(app),
        KeyCode::Char('Q') => navigation::handle_queue_request(app),
        KeyCode::Down => {
            // Load more builds when scrolling past the bottom.
            if app.build_history.nav.is_at_bottom()
                && app.build_history.has_more
                && !app.build_history.loading_more
                && let (Some(def), Some(token)) = (
                    &app.build_history.selected_definition,
                    &app.build_history.continuation_token,
                )
            {
                return Action::FetchMoreBuilds {
                    definition_id: def.id,
                    continuation_token: token.clone(),
                };
            }
            app.current_nav_mut().down();
            Action::None
        }
        KeyCode::End => {
            app.current_nav_mut().end();
            // Load more builds when jumping to end.
            if app.build_history.nav.is_at_bottom()
                && app.build_history.has_more
                && !app.build_history.loading_more
                && let (Some(def), Some(token)) = (
                    &app.build_history.selected_definition,
                    &app.build_history.continuation_token,
                )
            {
                return Action::FetchMoreBuilds {
                    definition_id: def.id,
                    continuation_token: token.clone(),
                };
            }
            Action::None
        }
        KeyCode::Left => {
            app.go_back();
            Action::None
        }
        KeyCode::Right => handle_enter_build_history(app),
        KeyCode::Enter => handle_enter_build_history(app),
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}

/// Handles the Enter key on build history, drilling into the log viewer.
fn handle_enter_build_history(app: &mut App) -> Action {
    if let Some(build) = app
        .build_history
        .builds
        .get(app.build_history.nav.index())
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
    fn right_arrow_on_build_history_drills_into_log_viewer() {
        let mut app = make_app();
        let def = make_definition(1, "P", "\\");
        app.navigate_to_build_history(def);
        app.build_history.builds = vec![make_build(
            300,
            BuildStatus::Completed,
            Some(BuildResult::Succeeded),
        )];
        app.build_history.nav.set_len(1);

        let action = handle_key(&mut app, key(KeyCode::Right));
        assert_eq!(app.view, View::LogViewer);
        assert!(matches!(action, Action::FetchTimeline(_)));
    }

    #[test]
    fn left_arrow_on_build_history_goes_back() {
        let mut app = make_app();
        app.view = View::Pipelines;
        let def = make_definition(1, "P", "\\");
        app.navigate_to_build_history(def);
        assert_eq!(app.view, View::BuildHistory);

        handle_key(&mut app, key(KeyCode::Left));
        assert_eq!(app.view, View::Pipelines);
    }
}
