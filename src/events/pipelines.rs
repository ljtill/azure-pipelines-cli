//! Event handling for the folder-based pipelines view.

use crossterm::event::{KeyCode, KeyEvent};

use super::Action;
use super::navigation;
use crate::components::pipelines::PipelineRow;
use crate::state::{App, InputMode};

/// Handles key events specific to the pipelines view.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('/') => {
            tracing::debug!(view = ?app.view, "entering search mode");
            app.search.mode = InputMode::Search;
            Action::None
        }
        KeyCode::Left => handle_left(app),
        KeyCode::Right => handle_right(app),
        KeyCode::Enter => handle_enter(app),
        KeyCode::Char(' ') => handle_space(app),
        KeyCode::Char('p') => handle_pin(app),
        KeyCode::Char('n') => navigation::handle_queue_request(app),
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}

/// Collapses a folder or navigates to the parent folder.
fn handle_left(app: &mut App) -> Action {
    let idx = app.pipelines.nav.index();
    if app.pipelines.is_folder_header(idx) {
        if app.pipelines.collapse_folder_at(idx) {
            app.rebuild_pipelines();
        }
    } else if let Some(folder_idx) = app.pipelines.find_parent_folder_index(idx) {
        if app.pipelines.collapse_folder_at(folder_idx) {
            app.rebuild_pipelines();
        }
        app.pipelines.nav.set_index(folder_idx);
    }
    Action::None
}

/// Expands a folder or drills into build history.
fn handle_right(app: &mut App) -> Action {
    let idx = app.pipelines.nav.index();
    if app.pipelines.is_folder_header(idx) {
        if app.pipelines.expand_folder_at(idx) {
            app.rebuild_pipelines();
        }
        Action::None
    } else {
        handle_drill_in(app)
    }
}

/// Toggles folder collapse or drills into pipeline build history.
fn handle_enter(app: &mut App) -> Action {
    let idx = app.pipelines.nav.index();
    if app.pipelines.is_folder_header(idx) {
        if app.pipelines.toggle_folder_at(idx) {
            app.rebuild_pipelines();
        }
        Action::None
    } else {
        handle_drill_in(app)
    }
}

/// Drills into build history for the selected pipeline.
fn handle_drill_in(app: &mut App) -> Action {
    if let Some(def) = app
        .pipelines
        .definition_at(app.pipelines.nav.index())
        .cloned()
    {
        let def_id = def.id;
        app.navigate_to_build_history(def);
        Action::FetchBuildHistory(def_id)
    } else {
        Action::None
    }
}

/// Toggles multi-select on the current pipeline row.
fn handle_space(app: &mut App) -> Action {
    let idx = app.pipelines.nav.index();
    if let Some(PipelineRow::Pipeline { definition, .. }) = app.pipelines.rows.get(idx) {
        let id = definition.id;
        if app.pipelines.selected.contains(&id) {
            app.pipelines.selected.remove(&id);
        } else {
            app.pipelines.selected.insert(id);
        }
    }
    Action::None
}

/// Pins or unpins the selected (or cursor) pipelines.
fn handle_pin(app: &mut App) -> Action {
    // Collect definition IDs to toggle.
    let ids_to_toggle: Vec<u32> = if app.pipelines.selected.is_empty() {
        // Use cursor item.
        app.pipelines
            .definition_at(app.pipelines.nav.index())
            .map(|def| vec![def.id])
            .unwrap_or_default()
    } else {
        app.pipelines.selected.iter().copied().collect()
    };

    if ids_to_toggle.is_empty() {
        return Action::None;
    }

    // Toggle: if all are already pinned, unpin them; otherwise pin them.
    let all_pinned = ids_to_toggle
        .iter()
        .all(|id| app.filters.pinned_definition_ids.contains(id));

    if all_pinned {
        app.filters
            .pinned_definition_ids
            .retain(|id| !ids_to_toggle.contains(id));
        app.notifications
            .success(format!("Unpinned {} pipeline(s)", ids_to_toggle.len()));
    } else {
        for id in &ids_to_toggle {
            if !app.filters.pinned_definition_ids.contains(id) {
                app.filters.pinned_definition_ids.push(*id);
            }
        }
        app.notifications
            .success(format!("Pinned {} pipeline(s)", ids_to_toggle.len()));
    }

    app.pipelines.selected.clear();

    // Persist to config.
    let config = app.current_config();
    if let Err(e) = config.save_blocking(&app.config_path) {
        tracing::error!(%e, "failed to save config after pin toggle");
        app.notifications
            .error(format!("Failed to save config: {e}"));
    }

    // Rebuild to update pin indicators.
    app.rebuild_pipelines();
    app.rebuild_dashboard();

    Action::None
}

#[cfg(test)]
mod tests {
    use crate::events::{Action, handle_key};
    use crate::state::View;
    use crate::test_helpers::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn enter_on_pipeline_drills_into_build_history() {
        let mut app = make_app();
        app.view = View::Pipelines;
        // Navigate past the folder header to the first pipeline.
        app.pipelines.nav.set_index(1);

        let action = handle_key(&mut app, key(KeyCode::Enter));
        assert_eq!(app.view, View::BuildHistory);
        assert!(matches!(action, Action::FetchBuildHistory(_)));
    }

    #[test]
    fn enter_on_folder_header_toggles_collapse() {
        let mut app = make_app();
        app.view = View::Pipelines;
        // Index 0 is a folder header.
        let rows_before = app.pipelines.rows.len();
        handle_key(&mut app, key(KeyCode::Enter));
        // Toggling collapse should change row count.
        assert_ne!(app.pipelines.rows.len(), rows_before);
    }

    #[test]
    fn left_on_folder_collapses() {
        let mut app = make_app();
        app.view = View::Pipelines;
        let rows_before = app.pipelines.rows.len();
        handle_key(&mut app, key(KeyCode::Left));
        assert!(app.pipelines.rows.len() <= rows_before);
    }
}
