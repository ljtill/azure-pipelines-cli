//! Keyboard event handling for the personal Boards list sub-views.

use crossterm::event::{KeyCode, KeyEvent};

use super::Action;
use super::navigation;
use crate::state::{App, InputMode};

/// Handles keys specific to the personal Boards sub-views (Assigned / Created).
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('/') => {
            app.search.mode = InputMode::Search;
            Action::None
        }
        KeyCode::Char('o') | KeyCode::Enter => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}
