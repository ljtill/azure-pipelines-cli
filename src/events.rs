use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, ConfirmAction, ConfirmPrompt, InputMode, View};

/// The action requested by the user after handling a key event.
#[derive(Debug)]
pub enum Action {
    None,
    Quit,
    ForceRefresh,
    FetchBuildHistory(u32),
    FetchBuildLog {
        build_id: u32,
        log_id: u32,
    },
    FetchTimeline(u32),
    FollowLatest,
    OpenInBrowser(String),
    CancelBuild(u32),
    CancelBuilds(Vec<u32>),
    RetryStage {
        build_id: u32,
        stage_ref_name: String,
    },
    QueuePipeline(u32),
    ApproveCheck(String),
    RejectCheck(String),
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    // Ctrl+C always quits
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Action::Quit;
    }

    // Confirmation prompt — only accept y/n/Esc
    if app.confirm_prompt.is_some() {
        return handle_confirm_key(app, key);
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
        KeyCode::Char('q') => match app.view {
            View::Dashboard | View::Pipelines | View::ActiveRuns => Action::Quit,
            _ => {
                app.go_back();
                Action::None
            }
        },
        KeyCode::Char('?') => {
            app.show_help = true;
            Action::None
        }
        KeyCode::Char('r') => Action::ForceRefresh,
        KeyCode::Char('f') if app.view == View::LogViewer => {
            app.log_viewer.follow_mode = true;
            Action::FollowLatest
        }
        KeyCode::Char('/') if app.view == View::Pipelines || app.view == View::ActiveRuns => {
            app.input_mode = InputMode::Search;
            Action::None
        }

        // Open in browser
        KeyCode::Char('o') => handle_open_in_browser(app),

        // Multi-select toggle in Active Runs
        KeyCode::Char(' ') if app.view == View::ActiveRuns => {
            if let Some(build) = app.filtered_active_builds.get(app.active_runs_nav.index()) {
                let id = build.id;
                if !app.selected_builds.remove(&id) {
                    app.selected_builds.insert(id);
                }
            }
            Action::None
        }

        // Cancel build(s)
        KeyCode::Char('c') if app.view == View::LogViewer || app.view == View::ActiveRuns => {
            handle_cancel_request(app)
        }

        // Retry stage (Shift+R)
        KeyCode::Char('R') if app.view == View::LogViewer => handle_retry_request(app),

        // Approve check (Shift+A)
        KeyCode::Char('A') if app.view == View::LogViewer => handle_approve_request(app),

        // Reject check (Shift+D)
        KeyCode::Char('D') if app.view == View::LogViewer => handle_reject_request(app),

        // Queue pipeline (Shift+Q)
        KeyCode::Char('Q')
            if app.view == View::Dashboard
                || app.view == View::Pipelines
                || app.view == View::BuildHistory =>
        {
            handle_queue_request(app)
        }

        // Tab switching
        KeyCode::Char('1') => {
            app.search_query.clear();
            app.view = View::Dashboard;
            Action::None
        }
        KeyCode::Char('2') => {
            app.search_query.clear();
            app.view = View::Pipelines;
            app.rebuild_filtered_pipelines();
            Action::None
        }
        KeyCode::Char('3') => {
            app.search_query.clear();
            app.view = View::ActiveRuns;
            app.rebuild_filtered_active_builds();
            Action::None
        }

        // Navigation
        KeyCode::Up => {
            app.current_nav_mut().up();
            Action::None
        }
        KeyCode::Down => {
            app.current_nav_mut().down();
            Action::None
        }

        // Left/Right for folder collapse/expand on Dashboard
        KeyCode::Left if app.view == View::Dashboard => {
            let idx = app.dashboard_nav.index();
            if app.is_folder_header(idx) {
                app.collapse_folder_at(idx);
            } else if let Some(folder_idx) = app.find_parent_folder_index(idx) {
                app.collapse_folder_at(folder_idx);
                app.dashboard_nav.set_index(folder_idx);
            }
            Action::None
        }
        KeyCode::Right if app.view == View::Dashboard => {
            let idx = app.dashboard_nav.index();
            if app.is_folder_header(idx) {
                app.expand_folder_at(idx);
            } else {
                return handle_enter(app);
            }
            Action::None
        }

        // Left/Right for timeline tree collapse/expand in LogViewer
        KeyCode::Left if app.view == View::LogViewer => {
            let idx = app.log_viewer.log_entries_nav.index();
            match app.timeline_row_kind(idx) {
                Some("stage") => {
                    app.collapse_timeline_node(idx);
                }
                Some("job") => {
                    if !app.collapse_timeline_node(idx)
                        && let Some(parent_idx) = app.find_timeline_parent_index(idx)
                    {
                        app.log_viewer.log_entries_nav.set_index(parent_idx);
                    }
                }
                Some("task") => {
                    if let Some(parent_idx) = app.find_timeline_parent_index(idx) {
                        app.log_viewer.log_entries_nav.set_index(parent_idx);
                    }
                }
                _ => {}
            }
            Action::None
        }
        KeyCode::Right if app.view == View::LogViewer => {
            let idx = app.log_viewer.log_entries_nav.index();
            match app.timeline_row_kind(idx) {
                Some("stage") | Some("job") => {
                    app.expand_timeline_node(idx);
                }
                Some("task") => {
                    return handle_enter(app);
                }
                _ => {}
            }
            Action::None
        }

        KeyCode::Home => {
            app.current_nav_mut().home();
            Action::None
        }
        KeyCode::End => {
            app.current_nav_mut().end();
            Action::None
        }

        KeyCode::Esc => {
            app.go_back();
            Action::None
        }

        KeyCode::Enter => handle_enter(app),

        // Log viewer scroll
        KeyCode::PageUp if app.view == View::LogViewer => {
            app.log_viewer.log_auto_scroll = false;
            app.log_viewer.log_scroll_offset = app.log_viewer.log_scroll_offset.saturating_sub(20);
            Action::None
        }
        KeyCode::PageDown if app.view == View::LogViewer => {
            app.log_viewer.log_scroll_offset = app.log_viewer.log_scroll_offset.saturating_add(20);
            Action::None
        }

        _ => Action::None,
    }
}

fn handle_confirm_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
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
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.confirm_prompt = None;
            Action::None
        }
        _ => Action::None,
    }
}

fn handle_open_in_browser(app: &App) -> Action {
    let url = match app.view {
        View::Dashboard => {
            if let Some(crate::app::DashboardRow::Pipeline { definition, .. }) =
                app.dashboard_rows.get(app.dashboard_nav.index())
            {
                Some(app.endpoints_web_definition(definition.id))
            } else {
                None
            }
        }
        View::Pipelines => app
            .filtered_pipelines
            .get(app.pipelines_nav.index())
            .map(|def| app.endpoints_web_definition(def.id)),
        View::ActiveRuns => app
            .filtered_active_builds
            .get(app.active_runs_nav.index())
            .map(|b| app.endpoints_web_build(b.id)),
        View::BuildHistory => app
            .definition_builds
            .get(app.builds_nav.index())
            .map(|b| app.endpoints_web_build(b.id)),
        View::LogViewer => app
            .log_viewer
            .selected_build
            .as_ref()
            .map(|b| app.endpoints_web_build(b.id)),
    };

    match url {
        Some(url) => Action::OpenInBrowser(url),
        None => Action::None,
    }
}

fn handle_cancel_request(app: &mut App) -> Action {
    // Batch cancel: if items are selected in Active Runs, cancel all of them
    if app.view == View::ActiveRuns && !app.selected_builds.is_empty() {
        let count = app.selected_builds.len();
        let build_ids: Vec<u32> = app.selected_builds.iter().copied().collect();
        app.confirm_prompt = Some(ConfirmPrompt {
            message: format!("Cancel {} selected build(s)?  [y/N]", count),
            action: ConfirmAction::CancelBuilds { build_ids },
        });
        return Action::None;
    }

    // Single cancel: cursor item
    let build = match app.view {
        View::LogViewer => app.log_viewer.selected_build.as_ref(),
        View::ActiveRuns => app.filtered_active_builds.get(app.active_runs_nav.index()),
        _ => None,
    };

    if let Some(build) = build
        && build.status.is_in_progress()
    {
        app.confirm_prompt = Some(ConfirmPrompt {
            message: format!("Cancel build #{}?  [y/N]", build.build_number),
            action: ConfirmAction::CancelBuild { build_id: build.id },
        });
    }
    Action::None
}

fn handle_retry_request(app: &mut App) -> Action {
    let idx = app.log_viewer.log_entries_nav.index();
    if app.timeline_row_kind(idx) != Some("stage") {
        return Action::None;
    }
    let stage_ref_name = match app.timeline_stage_ref_name(idx) {
        Some(name) => name,
        None => return Action::None,
    };
    let build_id = match &app.log_viewer.selected_build {
        Some(b) => b.id,
        None => return Action::None,
    };
    let build_number = app
        .log_viewer
        .selected_build
        .as_ref()
        .map(|b| b.build_number.as_str())
        .unwrap_or("?");
    let stage_name = match &app.log_viewer.timeline_rows.get(idx) {
        Some(crate::app::TimelineRow::Stage { name, .. }) => name.clone(),
        _ => stage_ref_name.clone(),
    };

    app.confirm_prompt = Some(ConfirmPrompt {
        message: format!(
            "Retry stage \"{}\" in build #{}?  [y/N]",
            stage_name, build_number
        ),
        action: ConfirmAction::RetryStage {
            build_id,
            stage_ref_name,
        },
    });
    Action::None
}

fn handle_queue_request(app: &mut App) -> Action {
    let (def_id, def_name) = match app.view {
        View::Dashboard => {
            if let Some(crate::app::DashboardRow::Pipeline { definition, .. }) =
                app.dashboard_rows.get(app.dashboard_nav.index())
            {
                (definition.id, definition.name.clone())
            } else {
                return Action::None;
            }
        }
        View::Pipelines => {
            if let Some(def) = app.filtered_pipelines.get(app.pipelines_nav.index()) {
                (def.id, def.name.clone())
            } else {
                return Action::None;
            }
        }
        View::BuildHistory => {
            if let Some(def) = &app.selected_definition {
                (def.id, def.name.clone())
            } else {
                return Action::None;
            }
        }
        _ => return Action::None,
    };

    app.confirm_prompt = Some(ConfirmPrompt {
        message: format!("Queue new run of \"{}\"?  [y/N]", def_name),
        action: ConfirmAction::QueuePipeline {
            definition_id: def_id,
        },
    });
    Action::None
}

fn handle_approve_request(app: &mut App) -> Action {
    let idx = app.log_viewer.log_entries_nav.index();
    if app.timeline_row_kind(idx) != Some("checkpoint") {
        return Action::None;
    }
    let approval_id = match app.timeline_approval_id(idx) {
        Some(id) => id,
        None => return Action::None,
    };
    let name = match &app.log_viewer.timeline_rows.get(idx) {
        Some(crate::app::TimelineRow::Checkpoint { name, .. }) => name.clone(),
        _ => "check".to_string(),
    };
    app.confirm_prompt = Some(ConfirmPrompt {
        message: format!("Approve \"{}\"?  [y/N]", name),
        action: ConfirmAction::ApproveCheck { approval_id },
    });
    Action::None
}

fn handle_reject_request(app: &mut App) -> Action {
    let idx = app.log_viewer.log_entries_nav.index();
    if app.timeline_row_kind(idx) != Some("checkpoint") {
        return Action::None;
    }
    let approval_id = match app.timeline_approval_id(idx) {
        Some(id) => id,
        None => return Action::None,
    };
    let name = match &app.log_viewer.timeline_rows.get(idx) {
        Some(crate::app::TimelineRow::Checkpoint { name, .. }) => name.clone(),
        _ => "check".to_string(),
    };
    app.confirm_prompt = Some(ConfirmPrompt {
        message: format!("Reject \"{}\"?  [y/N]", name),
        action: ConfirmAction::RejectCheck { approval_id },
    });
    Action::None
}

fn handle_search_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
            app.search_query.clear();
            rebuild_search_results(app);
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            rebuild_search_results(app);
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
            rebuild_search_results(app);
        }
        _ => {}
    }
    Action::None
}

fn rebuild_search_results(app: &mut App) {
    match app.view {
        View::Pipelines => {
            app.rebuild_filtered_pipelines();
            app.pipelines_nav.set_index(0);
        }
        View::ActiveRuns => {
            app.rebuild_filtered_active_builds();
            app.active_runs_nav.set_index(0);
        }
        _ => {}
    }
}

fn handle_enter(app: &mut App) -> Action {
    match app.view {
        View::Dashboard => {
            if let Some(row) = app.dashboard_rows.get(app.dashboard_nav.index()) {
                match row {
                    crate::app::DashboardRow::FolderHeader { .. } => {
                        app.toggle_folder_at(app.dashboard_nav.index());
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
            if let Some(def) = app
                .filtered_pipelines
                .get(app.pipelines_nav.index())
                .cloned()
            {
                let def_id = def.id;
                app.navigate_to_build_history(def);
                Action::FetchBuildHistory(def_id)
            } else {
                Action::None
            }
        }
        View::ActiveRuns => {
            if let Some(build) = app
                .filtered_active_builds
                .get(app.active_runs_nav.index())
                .cloned()
            {
                let build_id = build.id;
                app.navigate_to_log_viewer(build);
                Action::FetchTimeline(build_id)
            } else {
                Action::None
            }
        }
        View::BuildHistory => {
            if let Some(build) = app.definition_builds.get(app.builds_nav.index()).cloned() {
                let build_id = build.id;
                app.navigate_to_log_viewer(build);
                Action::FetchTimeline(build_id)
            } else {
                Action::None
            }
        }
        View::LogViewer => {
            let idx = app.log_viewer.log_entries_nav.index();
            match app.timeline_row_kind(idx) {
                Some("stage") | Some("job") => {
                    app.toggle_timeline_node(idx);
                    Action::None
                }
                Some("task") => {
                    app.log_viewer.follow_mode = false;
                    if let Some(log_id) = app.timeline_task_log_id(idx)
                        && let Some(build) = &app.log_viewer.selected_build
                    {
                        return Action::FetchBuildLog {
                            build_id: build.id,
                            log_id,
                        };
                    }
                    Action::None
                }
                _ => Action::None,
            }
        }
    }
}
