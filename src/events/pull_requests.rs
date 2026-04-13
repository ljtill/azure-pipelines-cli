//! Keyboard event handling for the Pull Requests list view.

use crossterm::event::{KeyCode, KeyEvent};

use super::Action;
use super::navigation;
use crate::state::{App, InputMode};

/// Handles keys specific to the Pull Requests view.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Tab => {
            app.pull_requests.mode = app.pull_requests.mode.next();
            tracing::info!(mode = ?app.pull_requests.mode, "cycling PR view mode");
            Action::FetchPullRequests
        }
        KeyCode::Char('/') => {
            app.search.mode = InputMode::Search;
            Action::None
        }
        KeyCode::Right | KeyCode::Enter => {
            // Drill into PR detail — will be wired in Phase 3.
            Action::None
        }
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}
