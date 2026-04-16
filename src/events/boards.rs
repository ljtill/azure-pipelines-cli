//! Keyboard event handling for the Azure Boards backlog view.

use crossterm::event::{KeyCode, KeyEvent};

use super::Action;
use super::navigation;
use crate::state::{App, InputMode};

/// Handles keys specific to the Boards view.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('/') => {
            app.search.mode = InputMode::Search;
            Action::None
        }
        KeyCode::Left => {
            let index = app.boards.nav.index();
            if !app.boards.collapse_row(index, &app.search.query)
                && let Some(parent_index) = app.boards.parent_index(index)
            {
                app.boards.nav.set_index(parent_index);
            }
            Action::None
        }
        KeyCode::Right => {
            app.boards
                .expand_row(app.boards.nav.index(), &app.search.query);
            Action::None
        }
        KeyCode::Enter => {
            app.boards
                .toggle_row(app.boards.nav.index(), &app.search.query);
            Action::None
        }
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}
