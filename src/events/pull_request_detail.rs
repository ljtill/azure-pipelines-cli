//! Keyboard event handling for the Pull Request detail view.

use crossterm::event::{KeyCode, KeyEvent};

use super::Action;
use super::navigation;
use crate::state::App;

/// Handles keys specific to the Pull Request detail view.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Left => {
            app.go_back();
            Action::None
        }
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}
