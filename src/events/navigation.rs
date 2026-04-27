//! Cross-view navigation actions like opening in browser, cancelling, and queueing builds.

use super::Action;
use crate::state::{App, ConfirmAction, ConfirmPrompt, View};

/// Opens the currently selected item in the default web browser.
pub fn handle_open_in_browser(app: &App) -> Action {
    let url = match app.view {
        View::Dashboard => {
            let idx = app.dashboard.nav.index();
            app.dashboard
                .pinned_definition_at(idx, &app.core.data.definitions)
                .map(|def| app.endpoints_web_definition(def.id))
                .or_else(|| {
                    app.dashboard
                        .pull_request_at(idx, &app.dashboard_pull_requests)
                        .and_then(|pr| {
                            let repo_name = pr.repo_name();
                            if repo_name.is_empty() {
                                None
                            } else {
                                Some(app.endpoints_web_pull_request(repo_name, pr.pull_request_id))
                            }
                        })
                })
                .or_else(|| {
                    app.dashboard
                        .work_item_at(idx, &app.dashboard_work_items, &app.pinned_work_items)
                        .map(|wi| app.endpoints_web_work_item(wi.id))
                })
        }
        View::Pipelines => app
            .pipelines
            .definition_at(app.pipelines.nav.index(), &app.core.data.definitions)
            .map(|def| app.endpoints_web_definition(def.id)),
        View::ActiveRuns => app
            .active_runs
            .build_at(&app.core.data.active_builds, app.active_runs.nav.index())
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
        View::PullRequestsCreatedByMe
        | View::PullRequestsAssignedToMe
        | View::PullRequestsAllActive => app
            .pull_requests
            .pull_request_at(app.pull_requests.nav.index())
            .and_then(|pr| {
                let repo_name = pr.repo_name();
                if repo_name.is_empty() {
                    None
                } else {
                    Some(app.endpoints_web_pull_request(repo_name, pr.pull_request_id))
                }
            }),
        View::PullRequestDetail => app
            .pull_request_detail
            .pull_request
            .as_ref()
            .and_then(|pr| {
                let repo_name = pr.repo_name();
                if repo_name.is_empty() {
                    None
                } else {
                    Some(app.endpoints_web_pull_request(repo_name, pr.pull_request_id))
                }
            }),
        View::Boards => app
            .boards
            .selected_work_item_id()
            .map(|work_item_id| app.endpoints_web_work_item(work_item_id)),
        View::BoardsAssignedToMe | View::BoardsCreatedByMe => app
            .my_work_items
            .list_for(app.view)
            .and_then(|list| list.row_at(list.nav.index()))
            .map(|row| app.endpoints_web_work_item(row.id)),
        View::WorkItemDetail => app
            .work_item_detail
            .work_item
            .as_ref()
            .map(|wi| app.endpoints_web_work_item(wi.id)),
    };

    url.map_or(Action::None, Action::OpenInBrowser)
}

/// Prompts the user to confirm cancellation of one or more builds.
pub fn handle_cancel_request(app: &mut App) -> Action {
    // Batch cancel: if items are selected in Active Runs, cancel all of them.
    if app.view == View::ActiveRuns && !app.active_runs.selected.is_empty() {
        let count = app.active_runs.selected.len();
        let build_ids: Vec<u32> = app.active_runs.selected.iter().copied().collect();
        app.confirm_prompt = Some(ConfirmPrompt {
            message: format!("Cancel {count} selected build(s)?  [y/N]"),
            action: ConfirmAction::CancelBuilds { build_ids },
        });
        return Action::None;
    }

    // Single cancel: cursor item.
    let build = match app.view {
        View::LogViewer => app.log_viewer.selected_build(),
        View::ActiveRuns => app
            .active_runs
            .build_at(&app.core.data.active_builds, app.active_runs.nav.index()),
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

/// Prompts the user to confirm queueing a new pipeline run.
pub fn handle_queue_request(app: &mut App) -> Action {
    let (def_id, def_name) = match app.view {
        View::Dashboard => {
            if let Some(def) = app
                .dashboard
                .pinned_definition_at(app.dashboard.nav.index(), &app.core.data.definitions)
            {
                (def.id, def.name.clone())
            } else {
                return Action::None;
            }
        }
        View::Pipelines => {
            if let Some(def) = app
                .pipelines
                .definition_at(app.pipelines.nav.index(), &app.core.data.definitions)
            {
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
        message: format!("Queue new run of \"{def_name}\"?  [y/N]"),
        action: ConfirmAction::QueuePipeline {
            definition_id: def_id,
        },
    });
    Action::None
}

/// Prompts the user to confirm deletion of retention leases for selected builds.
pub fn handle_delete_build_leases_request(app: &mut App) -> Action {
    // Batch delete: if builds are selected, collect leases for all of them.
    if !app.build_history.selected.is_empty() {
        let lease_ids: Vec<u32> = app
            .build_history
            .selected
            .iter()
            .flat_map(|&build_id| {
                app.core
                    .retention_leases
                    .leases_for_run(build_id)
                    .into_iter()
                    .map(|l| l.lease_id)
            })
            .collect();
        if lease_ids.is_empty() {
            app.notifications
                .error("Selected builds have no retention leases");
            return Action::None;
        }
        let count = lease_ids.len();
        app.confirm_prompt = Some(ConfirmPrompt {
            message: format!("Delete {count} lease(s) for selected builds?  [y/N]"),
            action: ConfirmAction::DeleteBuildLeases { lease_ids },
        });
        return Action::None;
    }

    // Single delete: cursor item.
    if let Some(build) = app.build_history.builds.get(app.build_history.nav.index()) {
        let lease_ids: Vec<u32> = app
            .core
            .retention_leases
            .leases_for_run(build.id)
            .iter()
            .map(|l| l.lease_id)
            .collect();
        if lease_ids.is_empty() {
            return Action::None;
        }
        let count = lease_ids.len();
        app.confirm_prompt = Some(ConfirmPrompt {
            message: format!(
                "Delete {} lease(s) for build #{}?  [y/N]",
                count, build.build_number
            ),
            action: ConfirmAction::DeleteBuildLeases { lease_ids },
        });
    }
    Action::None
}
