//! Keyboard event handling for the work item detail view.

use crossterm::event::{KeyCode, KeyEvent};

use super::Action;
use super::navigation;
use crate::state::App;

/// Handles keys specific to the work item detail view.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Left | KeyCode::Esc | KeyCode::Char('q') => {
            app.go_back();
            Action::None
        }
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        KeyCode::Up => {
            app.work_item_detail.nav.up();
            Action::None
        }
        KeyCode::Down => {
            let count = app.work_item_detail.section_count();
            if count > 0 {
                app.work_item_detail.nav.set_len(count);
                app.work_item_detail.nav.down();
            }
            Action::None
        }
        _ => Action::None,
    }
}
