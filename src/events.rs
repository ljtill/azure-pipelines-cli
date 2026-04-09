use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, InputMode, View};

pub fn poll_event(timeout: Duration) -> Result<Option<Event>> {
    if event::poll(timeout)? {
        Ok(Some(event::read()?))
    } else {
        Ok(None)
    }
}

/// The action requested by the user after handling a key event.
pub enum Action {
    None,
    Quit,
    ForceRefresh,
    FetchBuildHistory(u32),
    FetchBuildLog { build_id: u32, log_id: u32 },
    FetchTimeline(u32),
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    // Ctrl+C always quits
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Action::Quit;
    }

    // Search mode input
    if app.input_mode == InputMode::Search {
        return handle_search_key(app, key);
    }

    // Help overlay — any key dismisses
    if app.show_help {
        app.show_help = false;
        return Action::None;
    }

    match key.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('?') => {
            app.show_help = true;
            Action::None
        }
        KeyCode::Char('r') => Action::ForceRefresh,
        KeyCode::Char('/') if app.view == View::Pipelines => {
            app.input_mode = InputMode::Search;
            Action::None
        }

        // Tab switching
        KeyCode::Char('1') => {
            app.view = View::Dashboard;
            Action::None
        }
        KeyCode::Char('2') => {
            app.view = View::Pipelines;
            app.rebuild_filtered_pipelines();
            Action::None
        }
        KeyCode::Char('3') => {
            app.view = View::ActiveRuns;
            Action::None
        }

        // Navigation
        KeyCode::Up => {
            app.move_up();
            Action::None
        }
        KeyCode::Down => {
            app.move_down();
            Action::None
        }

        // Left/Right for folder collapse/expand on Dashboard
        KeyCode::Left if app.view == View::Dashboard => {
            let idx = app.dashboard_index;
            if app.is_folder_header(idx) {
                // On a folder header: collapse it
                app.collapse_folder_at(idx);
            } else {
                // On a pipeline row: collapse parent folder and jump to its header
                if let Some(folder_idx) = app.find_parent_folder_index(idx) {
                    app.collapse_folder_at(folder_idx);
                    app.dashboard_index = folder_idx;
                }
            }
            Action::None
        }
        KeyCode::Right if app.view == View::Dashboard => {
            let idx = app.dashboard_index;
            if app.is_folder_header(idx) {
                // On a collapsed folder header: expand it
                app.expand_folder_at(idx);
            } else {
                // On a pipeline row: drill in (same as Enter)
                return handle_enter(app);
            }
            Action::None
        }

        KeyCode::Esc => {
            app.go_back();
            Action::None
        }

        KeyCode::Enter => handle_enter(app),

        // Log viewer scroll
        KeyCode::PageUp if app.view == View::LogViewer => {
            app.log_auto_scroll = false;
            app.log_scroll_offset = app.log_scroll_offset.saturating_sub(20);
            Action::None
        }
        KeyCode::PageDown if app.view == View::LogViewer => {
            app.log_scroll_offset = app.log_scroll_offset.saturating_add(20);
            Action::None
        }

        _ => Action::None,
    }
}

fn handle_search_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            app.search_query.clear();
            app.rebuild_filtered_pipelines();
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            app.rebuild_filtered_pipelines();
            app.pipelines_index = 0;
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
            app.rebuild_filtered_pipelines();
            app.pipelines_index = 0;
        }
        _ => {}
    }
    Action::None
}

fn handle_enter(app: &mut App) -> Action {
    match app.view {
        View::Dashboard => {
            if let Some(row) = app.dashboard_rows.get(app.dashboard_index) {
                match row {
                    crate::app::DashboardRow::FolderHeader { .. } => {
                        app.toggle_folder_at(app.dashboard_index);
                        Action::None
                    }
                    crate::app::DashboardRow::Pipeline { definition, .. } => {
                        let def_id = definition.id;
                        app.navigate_to_build_history(definition.clone());
                        Action::FetchBuildHistory(def_id)
                    }
                }
            } else {
                Action::None
            }
        }
        View::Pipelines => {
            if let Some(def) = app.filtered_pipelines.get(app.pipelines_index).cloned() {
                let def_id = def.id;
                app.navigate_to_build_history(def);
                Action::FetchBuildHistory(def_id)
            } else {
                Action::None
            }
        }
        View::ActiveRuns => {
            if let Some(build) = app.active_builds.get(app.active_runs_index).cloned() {
                let build_id = build.id;
                app.navigate_to_log_viewer(build);
                Action::FetchTimeline(build_id)
            } else {
                Action::None
            }
        }
        View::BuildHistory => {
            if let Some(build) = app.definition_builds.get(app.builds_index).cloned() {
                let build_id = build.id;
                app.navigate_to_log_viewer(build);
                Action::FetchTimeline(build_id)
            } else {
                Action::None
            }
        }
        View::LogViewer => {
            // Select a timeline record to view its log
            if let Some(timeline) = &app.build_timeline {
                let log_records: Vec<_> = timeline
                    .records
                    .iter()
                    .filter(|r| r.log.is_some())
                    .collect();
                if let Some(record) = log_records.get(app.log_entries_index) {
                    if let (Some(build), Some(log_ref)) = (&app.selected_build, &record.log) {
                        return Action::FetchBuildLog {
                            build_id: build.id,
                            log_id: log_ref.id,
                        };
                    }
                }
            }
            Action::None
        }
    }
}
