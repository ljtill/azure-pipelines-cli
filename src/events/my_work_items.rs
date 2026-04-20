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
        KeyCode::Enter => app
            .my_work_items
            .list_for(app.view)
            .and_then(|list| list.filtered.get(list.nav.index()))
            .map(|row| row.id)
            .map_or(Action::None, |id| {
                app.navigate_to_work_item_detail(id);
                Action::FetchWorkItemDetail { work_item_id: id }
            }),
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}
