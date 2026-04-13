//! Event handling for the dashboard view.

use crossterm::event::{KeyCode, KeyEvent};

use super::Action;
use super::navigation;
use crate::components::dashboard::DashboardRow;
use crate::state::App;

/// Handles key events specific to the dashboard view.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Right | KeyCode::Enter => handle_enter_dashboard(app),
        KeyCode::Char('Q') => navigation::handle_queue_request(app),
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}

/// Handles the Enter key on the dashboard, drilling into pipelines or PR detail.
fn handle_enter_dashboard(app: &mut App) -> Action {
    let idx = app.dashboard.nav.index();
    match app.dashboard.rows.get(idx).cloned() {
        Some(DashboardRow::PinnedPipeline { definition, .. }) => {
            let def_id = definition.id;
            app.navigate_to_build_history(definition);
            Action::FetchBuildHistory(def_id)
        }
        Some(DashboardRow::DashboardPullRequest { pull_request }) => {
            let repo_id = pull_request
                .repository
                .as_ref()
                .map_or(String::new(), |r| r.id.clone());
            let pr_id = pull_request.pull_request_id;
            app.navigate_to_pr_detail(&pull_request);
            Action::FetchPullRequestDetail { repo_id, pr_id }
        }
        _ => Action::None,
    }
}
