//! Event handling for the dashboard view.

use crossterm::event::{KeyCode, KeyEvent};

use super::Action;
use super::navigation;
use crate::state::App;

/// Handles key events specific to the dashboard view.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Left => {
            let idx = app.dashboard.nav.index();
            if app.dashboard.is_folder_header(idx) {
                if app.dashboard.collapse_folder_at(idx, &app.data.definitions) {
                    app.dashboard.rebuild(
                        &app.data.definitions,
                        &app.data.latest_builds_by_def,
                        &app.filters.folders,
                        &app.filters.definition_ids,
                    );
                }
            } else if let Some(folder_idx) = app.dashboard.find_parent_folder_index(idx) {
                if app
                    .dashboard
                    .collapse_folder_at(folder_idx, &app.data.definitions)
                {
                    app.dashboard.rebuild(
                        &app.data.definitions,
                        &app.data.latest_builds_by_def,
                        &app.filters.folders,
                        &app.filters.definition_ids,
                    );
                }
                app.dashboard.nav.set_index(folder_idx);
            }
            Action::None
        }
        KeyCode::Right => {
            let idx = app.dashboard.nav.index();
            if app.dashboard.is_folder_header(idx) {
                if app.dashboard.expand_folder_at(idx, &app.data.definitions) {
                    app.dashboard.rebuild(
                        &app.data.definitions,
                        &app.data.latest_builds_by_def,
                        &app.filters.folders,
                        &app.filters.definition_ids,
                    );
                }
            } else {
                return handle_enter_dashboard(app);
            }
            Action::None
        }
        KeyCode::Enter => handle_enter_dashboard(app),
        KeyCode::Char('Q') => navigation::handle_queue_request(app),
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}

/// Handles the Enter key on the dashboard, toggling folders or drilling into pipelines.
fn handle_enter_dashboard(app: &mut App) -> Action {
    if let Some(row) = app.dashboard.rows.get(app.dashboard.nav.index()) {
        match row {
            crate::state::DashboardRow::FolderHeader { .. } => {
                let idx = app.dashboard.nav.index();
                if app.dashboard.toggle_folder_at(idx, &app.data.definitions) {
                    app.dashboard.rebuild(
                        &app.data.definitions,
                        &app.data.latest_builds_by_def,
                        &app.filters.folders,
                        &app.filters.definition_ids,
                    );
                }
                Action::None
            }
            crate::state::DashboardRow::Pipeline { definition, .. } => {
                let def_id = definition.id;
                app.navigate_to_build_history(definition.clone());
                Action::FetchBuildHistory(def_id)
            }
        }
    } else {
        Action::None
    }
}
