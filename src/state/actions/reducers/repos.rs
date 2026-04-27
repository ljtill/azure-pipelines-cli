//! Repos reducer logic for pull request lists and detail views.

use crate::client::models::{PullRequest, PullRequestThread};
use crate::state::App;

pub(in crate::state::actions) fn pull_requests_loaded(
    app: &mut App,
    pull_requests: Vec<PullRequest>,
    generation: u64,
) {
    if generation != app.pull_requests.generation {
        tracing::debug!(
            generation,
            expected = app.pull_requests.generation,
            "dropping obsolete pull requests response"
        );
        return;
    }
    tracing::info!(count = pull_requests.len(), "pull requests loaded");
    let query = app.search.query.clone();
    app.pull_requests.set_data(pull_requests, &query);
}

pub(in crate::state::actions) fn pull_request_detail_loaded(
    app: &mut App,
    pull_request: PullRequest,
    threads: Vec<PullRequestThread>,
) {
    tracing::info!(
        pr_id = pull_request.pull_request_id,
        threads = threads.len(),
        "pull request detail loaded"
    );
    app.pull_request_detail.pull_request = Some(pull_request);
    app.pull_request_detail.threads = threads;
    app.pull_request_detail.loading = false;
    // Set nav length to number of display sections.
    let section_count = app.pull_request_detail.section_count();
    app.pull_request_detail.nav.set_len(section_count);
}
