use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use crate::app::{App, ConfirmAction, ConfirmPrompt, InputMode, TimelineRow, View};

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
    if app.search.mode == InputMode::Search {
        return handle_search_key(app, key);
    }

    // Help overlay — any key dismisses
    if app.show_help {
        app.show_help = false;
        return Action::None;
    }

    // Settings overlay — route keys to dedicated handler
    if app.show_settings {
        return handle_settings_key(app, key);
    }

    match key.code {
        KeyCode::Char('?') => {
            app.show_help = true;
            Action::None
        }
        KeyCode::Char(',') => {
            app.open_settings();
            Action::None
        }
        KeyCode::Char('r') => Action::ForceRefresh,
        KeyCode::Char('x')
            if app.view == View::Dashboard
                || app.view == View::Pipelines
                || app.view == View::ActiveRuns =>
        {
            app.notifications.clear();
            Action::None
        }
        KeyCode::Char('f') if app.view == View::LogViewer => {
            app.log_viewer.enter_follow_mode();
            Action::FollowLatest
        }
        KeyCode::Char('/') if app.view == View::Pipelines || app.view == View::ActiveRuns => {
            app.search.mode = InputMode::Search;
            Action::None
        }

        // Open in browser
        KeyCode::Char('o') => handle_open_in_browser(app),

        // Multi-select toggle in Active Runs
        KeyCode::Char(' ') if app.view == View::ActiveRuns => {
            if let Some(build) = app.active_runs.filtered.get(app.active_runs.nav.index()) {
                let id = build.id;
                if !app.active_runs.selected.remove(&id) {
                    app.active_runs.selected.insert(id);
                }
            }
            Action::None
        }

        // Cancel build(s)
        KeyCode::Char('c')
            if app.view == View::LogViewer
                || app.view == View::ActiveRuns
                || app.view == View::BuildHistory =>
        {
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
            app.search.query.clear();
            app.view = View::Dashboard;
            Action::None
        }
        KeyCode::Char('2') => {
            app.search.query.clear();
            app.view = View::Pipelines;
            app.pipelines.rebuild(
                &app.data.definitions,
                &app.filters.folders,
                &app.filters.definition_ids,
                &app.search.query,
            );
            Action::None
        }
        KeyCode::Char('3') => {
            app.search.query.clear();
            app.view = View::ActiveRuns;
            app.active_runs.rebuild(
                &app.data.active_builds,
                &app.filters.definition_ids,
                &app.search.query,
            );
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
        KeyCode::Right if app.view == View::Dashboard => {
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
                return handle_enter(app);
            }
            Action::None
        }

        // Left/Right for timeline tree collapse/expand in LogViewer
        KeyCode::Left if app.view == View::LogViewer => {
            let idx = app.log_viewer.nav().index();
            match app.log_viewer.timeline_row_kind(idx) {
                Some("stage") => {
                    app.log_viewer.collapse_timeline_node(idx);
                }
                Some("job") => {
                    if !app.log_viewer.collapse_timeline_node(idx)
                        && let Some(parent_idx) = app.log_viewer.find_timeline_parent_index(idx)
                    {
                        app.log_viewer.nav_mut().set_index(parent_idx);
                    }
                }
                Some("task") => {
                    if let Some(parent_idx) = app.log_viewer.find_timeline_parent_index(idx) {
                        app.log_viewer.nav_mut().set_index(parent_idx);
                    }
                }
                _ => {}
            }
            Action::None
        }
        KeyCode::Right if app.view == View::LogViewer => {
            let idx = app.log_viewer.nav().index();
            match app.log_viewer.timeline_row_kind(idx) {
                Some("stage") | Some("job") => {
                    app.log_viewer.expand_timeline_node(idx);
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

        KeyCode::Char('q') => match app.view {
            View::Dashboard => {
                app.confirm_prompt = Some(ConfirmPrompt {
                    message: "Quit? (y/n)".into(),
                    action: ConfirmAction::Quit,
                });
                Action::None
            }
            View::Pipelines | View::ActiveRuns => {
                app.search.mode = InputMode::Normal;
                app.search.query.clear();
                app.view = View::Dashboard;
                Action::None
            }
            _ => {
                app.go_back();
                Action::None
            }
        },

        KeyCode::Enter => handle_enter(app),

        // Log viewer scroll
        KeyCode::PageUp if app.view == View::LogViewer => {
            app.log_viewer.scroll_up(20);
            Action::None
        }
        KeyCode::PageDown if app.view == View::LogViewer => {
            app.log_viewer.scroll_down(20);
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
                ConfirmAction::Quit => Action::Quit,
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.confirm_prompt = None;
            Action::None
        }
        _ => Action::None,
    }
}

fn handle_settings_key(app: &mut App, key: KeyEvent) -> Action {
    let Some(settings) = app.settings.as_mut() else {
        return Action::None;
    };

    if settings.editing {
        // In field-edit mode
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

    // Normal settings navigation
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

fn handle_settings_save(app: &mut App) -> Action {
    if let Some(settings) = app.settings.as_ref() {
        match settings.save() {
            Ok(config) => {
                // Apply runtime-relevant changes
                app.filters.folders = config.filters.folders;
                app.filters.definition_ids = config.filters.definition_ids.clone();
                app.notifications_enabled = config.notifications.enabled;

                // Rebuild filtered views with new filters
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

fn handle_open_in_browser(app: &App) -> Action {
    let url = match app.view {
        View::Dashboard => {
            if let Some(crate::app::DashboardRow::Pipeline { definition, .. }) =
                app.dashboard.rows.get(app.dashboard.nav.index())
            {
                Some(app.endpoints_web_definition(definition.id))
            } else {
                None
            }
        }
        View::Pipelines => app
            .pipelines
            .filtered
            .get(app.pipelines.nav.index())
            .map(|def| app.endpoints_web_definition(def.id)),
        View::ActiveRuns => app
            .active_runs
            .filtered
            .get(app.active_runs.nav.index())
            .map(|b| app.endpoints_web_build(b.id)),
        View::BuildHistory => app
            .build_history
            .builds
            .get(app.build_history.nav.index())
            .map(|b| app.endpoints_web_build(b.id)),
        View::LogViewer => app
            .log_viewer
            .selected_build()
            .map(|b| app.endpoints_web_build(b.id)),
    };

    match url {
        Some(url) => Action::OpenInBrowser(url),
        None => Action::None,
    }
}

fn handle_cancel_request(app: &mut App) -> Action {
    // Batch cancel: if items are selected in Active Runs, cancel all of them
    if app.view == View::ActiveRuns && !app.active_runs.selected.is_empty() {
        let count = app.active_runs.selected.len();
        let build_ids: Vec<u32> = app.active_runs.selected.iter().copied().collect();
        app.confirm_prompt = Some(ConfirmPrompt {
            message: format!("Cancel {} selected build(s)?  [y/N]", count),
            action: ConfirmAction::CancelBuilds { build_ids },
        });
        return Action::None;
    }

    // Single cancel: cursor item
    let build = match app.view {
        View::LogViewer => app.log_viewer.selected_build(),
        View::ActiveRuns => app.active_runs.filtered.get(app.active_runs.nav.index()),
        View::BuildHistory => app.build_history.builds.get(app.build_history.nav.index()),
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
    let idx = app.log_viewer.nav().index();

    // Find the stage to retry: if cursor is on a stage, use it directly.
    // If on a job/task/checkpoint, walk up to the parent stage.
    let stage_idx = match app.log_viewer.timeline_row_kind(idx) {
        Some("stage") => Some(idx),
        Some("job") => app.log_viewer.find_timeline_parent_index(idx),
        Some("task") => app
            .log_viewer
            .find_timeline_parent_index(idx)
            .and_then(|job_idx| app.log_viewer.find_timeline_parent_index(job_idx)),
        Some("checkpoint") => {
            if let Some(TimelineRow::Checkpoint {
                parent_stage_id, ..
            }) = app.log_viewer.timeline_rows().get(idx)
            {
                let psid = parent_stage_id.clone();
                app.log_viewer
                    .timeline_rows()
                    .iter()
                    .position(|r| matches!(r, TimelineRow::Stage { id, .. } if *id == psid))
            } else {
                None
            }
        }
        _ => None,
    };

    let stage_idx = match stage_idx {
        Some(i) => i,
        None => return Action::None,
    };

    let stage_ref_name = match app.log_viewer.timeline_stage_ref_name(stage_idx) {
        Some(name) => name,
        None => return Action::None,
    };
    let build_id = match app.log_viewer.selected_build() {
        Some(b) => b.id,
        None => return Action::None,
    };
    let build_number = app
        .log_viewer
        .selected_build()
        .map(|b| b.build_number.as_str())
        .unwrap_or("?");
    let stage_name = match app.log_viewer.timeline_rows().get(stage_idx) {
        Some(TimelineRow::Stage { name, .. }) => name.clone(),
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
                app.dashboard.rows.get(app.dashboard.nav.index())
            {
                (definition.id, definition.name.clone())
            } else {
                return Action::None;
            }
        }
        View::Pipelines => {
            if let Some(def) = app.pipelines.filtered.get(app.pipelines.nav.index()) {
                (def.id, def.name.clone())
            } else {
                return Action::None;
            }
        }
        View::BuildHistory => {
            if let Some(def) = &app.build_history.selected_definition {
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
    let idx = app.log_viewer.nav().index();
    if app.log_viewer.timeline_row_kind(idx) != Some("checkpoint") {
        return Action::None;
    }
    let approval_id = match app.log_viewer.timeline_approval_id(idx) {
        Some(id) => id,
        None => return Action::None,
    };
    let name = match app.log_viewer.timeline_rows().get(idx) {
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
    let idx = app.log_viewer.nav().index();
    if app.log_viewer.timeline_row_kind(idx) != Some("checkpoint") {
        return Action::None;
    }
    let approval_id = match app.log_viewer.timeline_approval_id(idx) {
        Some(id) => id,
        None => return Action::None,
    };
    let name = match app.log_viewer.timeline_rows().get(idx) {
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

fn rebuild_search_results(app: &mut App) {
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
        _ => {}
    }
}

fn handle_enter(app: &mut App) -> Action {
    match app.view {
        View::Dashboard => {
            if let Some(row) = app.dashboard.rows.get(app.dashboard.nav.index()) {
                match row {
                    crate::app::DashboardRow::FolderHeader { .. } => {
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
                .pipelines
                .filtered
                .get(app.pipelines.nav.index())
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
                .active_runs
                .filtered
                .get(app.active_runs.nav.index())
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
            if let Some(build) = app
                .build_history
                .builds
                .get(app.build_history.nav.index())
                .cloned()
            {
                let build_id = build.id;
                app.navigate_to_log_viewer(build);
                Action::FetchTimeline(build_id)
            } else {
                Action::None
            }
        }
        View::LogViewer => {
            let idx = app.log_viewer.nav().index();
            match app.log_viewer.timeline_row_kind(idx) {
                Some("stage") | Some("job") => {
                    app.log_viewer.toggle_timeline_node(idx);
                    Action::None
                }
                Some("task") => {
                    app.log_viewer.enter_inspect_mode();
                    if let Some(log_id) = app.log_viewer.timeline_task_log_id(idx)
                        && let Some(build) = app.log_viewer.selected_build()
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

pub fn handle_mouse(app: &mut App, mouse: MouseEvent) -> Action {
    match mouse.kind {
        MouseEventKind::ScrollUp if app.view == View::LogViewer => {
            app.log_viewer.scroll_up(3);
            Action::None
        }
        MouseEventKind::ScrollDown if app.view == View::LogViewer => {
            app.log_viewer.scroll_down(3);
            Action::None
        }
        _ => Action::None,
    }
}
