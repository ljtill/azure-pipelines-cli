//! Shared event handlers used across multiple views.

use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::Action;
use crate::state::{App, ConfirmAction, InputMode, View};

/// Handles key input while a confirmation prompt is active.
pub fn handle_confirm_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('y' | 'Y') => {
            let prompt = app.confirm_prompt.take().unwrap();
            match prompt.action {
                ConfirmAction::CancelBuild { build_id } => Action::CancelBuild(build_id),
                ConfirmAction::CancelBuilds { build_ids } => Action::CancelBuilds(build_ids),
                ConfirmAction::RetryStage {
                    build_id,
                    stage_ref_name,
                } => Action::RetryStage {
                    build_id,
                    stage_ref_name,
                },
                ConfirmAction::QueuePipeline { definition_id } => {
                    Action::QueuePipeline(definition_id)
                }
                ConfirmAction::ApproveCheck { approval_id } => Action::ApproveCheck(approval_id),
                ConfirmAction::RejectCheck { approval_id } => Action::RejectCheck(approval_id),
                ConfirmAction::DeleteBuildLeases { lease_ids } => {
                    Action::DeleteRetentionLeases(lease_ids)
                }
                ConfirmAction::Quit => Action::Quit,
            }
        }
        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
            app.confirm_prompt = None;
            Action::None
        }
        _ => Action::None,
    }
}

/// Handles key input within the settings overlay.
pub fn handle_settings_key(app: &mut App, key: KeyEvent) -> Action {
    let Some(settings) = app.settings.as_mut() else {
        return Action::None;
    };

    if settings.editing {
        // In field-edit mode.
        match key.code {
            KeyCode::Esc => {
                settings.cancel_edit();
            }
            KeyCode::Enter => {
                settings.stop_edit();
            }
            KeyCode::Backspace => {
                settings.backspace();
            }
            KeyCode::Delete => {
                settings.delete();
            }
            KeyCode::Left => {
                settings.move_cursor_left();
            }
            KeyCode::Right => {
                settings.move_cursor_right();
            }
            KeyCode::Char(c) => {
                settings.insert_char(c);
            }
            _ => {}
        }
        return Action::None;
    }

    // Normal settings navigation.
    match key.code {
        KeyCode::Char('q') => {
            app.show_settings = false;
            app.settings = None;
        }
        KeyCode::Up => {
            settings.move_up();
        }
        KeyCode::Down => {
            settings.move_down();
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            settings.start_edit();
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return handle_settings_save(app);
        }
        KeyCode::Char('s') => {
            return handle_settings_save(app);
        }
        _ => {}
    }
    Action::None
}

/// Saves the current settings to disk and applies runtime changes.
pub fn handle_settings_save(app: &mut App) -> Action {
    if let Some(settings) = app.settings.as_ref() {
        match settings.save() {
            Ok(config) => {
                // Detect connection change (org/project).
                let new_label = format!(
                    "{} / {}",
                    config.azure_devops.organization, config.azure_devops.project
                );
                let needs_reload = new_label != app.org_project_label;

                // Apply runtime-relevant changes.
                app.filters.folders = config.filters.folders;
                app.filters.definition_ids = config.filters.definition_ids.clone();
                app.notifications_enabled = config.notifications.enabled;

                // Apply display settings live.
                app.refresh_interval = Duration::from_secs(config.display.refresh_interval_secs);
                app.log_refresh_interval =
                    Duration::from_secs(config.display.log_refresh_interval_secs);

                // Rebuild filtered views with new filters.
                app.dashboard.rebuild(
                    &app.data.definitions,
                    &app.data.latest_builds_by_def,
                    &app.filters.folders,
                    &app.filters.definition_ids,
                );
                app.pipelines.rebuild(
                    &app.data.definitions,
                    &app.filters.folders,
                    &app.filters.definition_ids,
                    &app.search.query,
                );
                app.active_runs.rebuild(
                    &app.data.active_builds,
                    &app.filters.definition_ids,
                    &app.search.query,
                );

                app.show_settings = false;
                app.settings = None;

                if needs_reload {
                    app.notifications.success("Settings saved — reloading…");
                    tracing::info!("settings saved, connection changed — reloading");
                    app.reload_requested = true;
                    return Action::Reload;
                }

                app.notifications.success("Settings saved");
                tracing::info!("settings saved to disk");
            }
            Err(e) => {
                app.notifications
                    .error(format!("Failed to save settings: {e}"));
                tracing::error!(%e, "failed to save settings");
            }
        }
    }
    app.show_settings = false;
    app.settings = None;
    Action::None
}

/// Handles key input while the search bar is active.
pub fn handle_search_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.search.mode = InputMode::Normal;
            app.search.query.clear();
            rebuild_search_results(app);
        }
        KeyCode::Enter => {
            app.search.mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            app.search.query.pop();
            rebuild_search_results(app);
        }
        KeyCode::Char(c) => {
            app.search.query.push(c);
            rebuild_search_results(app);
        }
        _ => {}
    }
    Action::None
}

/// Rebuilds the filtered list for the current view after a search query change.
pub fn rebuild_search_results(app: &mut App) {
    match app.view {
        View::Pipelines => {
            app.pipelines.rebuild(
                &app.data.definitions,
                &app.filters.folders,
                &app.filters.definition_ids,
                &app.search.query,
            );
            app.pipelines.nav.set_index(0);
        }
        View::ActiveRuns => {
            app.active_runs.rebuild(
                &app.data.active_builds,
                &app.filters.definition_ids,
                &app.search.query,
            );
            app.active_runs.nav.set_index(0);
        }
        // Pull Requests search will be wired in Phase 2.
        _ => {}
    }
}
