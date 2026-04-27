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
            let query = app.search.query.clone();
            if !app.shell.views.boards.collapse_row(index, &query)
                && let Some(parent_index) = app.shell.views.boards.parent_index(index)
            {
                app.shell.views.boards.nav.set_index(parent_index);
            }
            Action::None
        }
        KeyCode::Right => {
            let index = app.boards.nav.index();
            let query = app.search.query.clone();
            app.shell.views.boards.expand_row(index, &query);
            Action::None
        }
        KeyCode::Enter => {
            if let Some(id) = app.boards.selected_work_item_id() {
                app.navigate_to_work_item_detail(id);
                Action::FetchWorkItemDetail { work_item_id: id }
            } else {
                let index = app.boards.nav.index();
                let query = app.search.query.clone();
                app.shell.views.boards.toggle_row(index, &query);
                Action::None
            }
        }
        KeyCode::Char('p') => app
            .boards
            .selected_work_item_id()
            .map_or(Action::None, |id| {
                super::pins::toggle_work_item_pin(app, id)
            }),
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}
