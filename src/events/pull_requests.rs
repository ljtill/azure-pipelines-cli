//! Keyboard event handling for the Pull Requests list view.

use crossterm::event::{KeyCode, KeyEvent};

use super::Action;
use super::navigation;
use crate::state::{App, InputMode};

/// Handles keys specific to the Pull Requests view.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Tab => {
            app.pull_requests.mode = app.pull_requests.mode.next();
            tracing::info!(mode = ?app.pull_requests.mode, "cycling PR view mode");
            Action::FetchPullRequests
        }
        KeyCode::BackTab => {
            app.pull_requests.mode = app.pull_requests.mode.prev();
            tracing::info!(mode = ?app.pull_requests.mode, "cycling PR view mode backwards");
            Action::FetchPullRequests
        }
        KeyCode::Char('/') => {
            app.search.mode = InputMode::Search;
            Action::None
        }
        KeyCode::Right | KeyCode::Enter => {
            if let Some(pr) = app
                .pull_requests
                .filtered
                .get(app.pull_requests.nav.index())
                .cloned()
            {
                let repo_id = pr
                    .repository
                    .as_ref()
                    .map_or(String::new(), |r| r.id.clone());
                let pr_id = pr.pull_request_id;
                app.navigate_to_pr_detail(&pr);
                Action::FetchPullRequestDetail { repo_id, pr_id }
            } else {
                Action::None
            }
        }
        KeyCode::Char('o') => navigation::handle_open_in_browser(app),
        _ => Action::None,
    }
}
