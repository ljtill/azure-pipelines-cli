use std::path::PathBuf;

use azure_pipelines_cli::client::models::*;
use azure_pipelines_cli::events::{Action, handle_key};
use azure_pipelines_cli::state::{
    App, ConfirmAction, ConfirmPrompt, DashboardRow, InputMode, View,
};
use azure_pipelines_cli::test_helpers::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl_c() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
}

fn test_app() -> App {
    App::new("o", "p", &make_config(), PathBuf::from("/tmp/test.toml"))
}

// ---------------------------------------------------------------------------
// Tab switching
// ---------------------------------------------------------------------------

#[test]
fn key_1_switches_to_dashboard() {
    let mut app = test_app();
    app.view = View::Pipelines;
    let action = handle_key(&mut app, key(KeyCode::Char('1')));
    assert_eq!(app.view, View::Dashboard);
    assert!(matches!(action, Action::None));
}

#[test]
fn key_2_switches_to_pipelines() {
    let mut app = test_app();
    app.data.definitions = vec![make_definition(1, "Pipeline 1", "\\")];
    let action = handle_key(&mut app, key(KeyCode::Char('2')));
    assert_eq!(app.view, View::Pipelines);
    assert!(!app.pipelines.rows.is_empty());
    assert!(matches!(action, Action::None));
}

#[test]
fn key_3_switches_to_active_runs() {
    let mut app = test_app();
    let action = handle_key(&mut app, key(KeyCode::Char('3')));
    assert_eq!(app.view, View::ActiveRuns);
    assert!(matches!(action, Action::None));
}

// ---------------------------------------------------------------------------
// Quit / Esc behavior
// ---------------------------------------------------------------------------

#[test]
fn esc_shows_quit_prompt_on_dashboard() {
    let mut app = test_app();
    assert_eq!(app.view, View::Dashboard);
    let action = handle_key(&mut app, key(KeyCode::Char('q')));
    assert!(app.confirm_prompt.is_some());
    assert!(matches!(
        app.confirm_prompt.as_ref().unwrap().action,
        ConfirmAction::Quit
    ));
    assert!(matches!(action, Action::None));
}

#[test]
fn esc_navigates_to_dashboard_from_pipelines() {
    let mut app = test_app();
    app.view = View::Pipelines;
    let action = handle_key(&mut app, key(KeyCode::Char('q')));
    assert_eq!(app.view, View::Dashboard);
    assert!(matches!(action, Action::None));
}

#[test]
fn esc_navigates_to_dashboard_from_active_runs() {
    let mut app = test_app();
    app.view = View::ActiveRuns;
    let action = handle_key(&mut app, key(KeyCode::Char('q')));
    assert_eq!(app.view, View::Dashboard);
    assert!(matches!(action, Action::None));
}

#[test]
fn esc_goes_back_from_build_history_to_dashboard() {
    let mut app = test_app();
    app.view = View::BuildHistory;
    app.build_history.return_to = Some(View::Dashboard);
    let action = handle_key(&mut app, key(KeyCode::Char('q')));
    assert_eq!(app.view, View::Dashboard);
    assert!(matches!(action, Action::None));
}

#[test]
fn esc_goes_back_from_log_viewer() {
    let mut app = test_app();
    let build = make_build(1, BuildStatus::InProgress, None);
    app.navigate_to_log_viewer(build);
    assert_eq!(app.view, View::LogViewer);

    let action = handle_key(&mut app, key(KeyCode::Char('q')));
    assert_ne!(app.view, View::LogViewer);
    assert!(matches!(action, Action::None));
}

#[test]
fn confirm_quit_y_quits() {
    let mut app = test_app();
    app.confirm_prompt = Some(ConfirmPrompt {
        message: "Quit? (y/n)".into(),
        action: ConfirmAction::Quit,
    });
    let action = handle_key(&mut app, key(KeyCode::Char('y')));
    assert!(matches!(action, Action::Quit));
}

#[test]
fn ctrl_c_always_quits() {
    for view in [
        View::Dashboard,
        View::Pipelines,
        View::ActiveRuns,
        View::BuildHistory,
        View::LogViewer,
    ] {
        let mut app = test_app();
        app.view = view;
        let action = handle_key(&mut app, ctrl_c());
        assert!(
            matches!(action, Action::Quit),
            "Ctrl+C should quit from {view:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Help
// ---------------------------------------------------------------------------

#[test]
fn question_mark_shows_help() {
    let mut app = test_app();
    let action = handle_key(&mut app, key(KeyCode::Char('?')));
    assert!(app.show_help);
    assert!(matches!(action, Action::None));
}

#[test]
fn any_key_dismisses_help() {
    let mut app = test_app();
    app.show_help = true;
    let action = handle_key(&mut app, key(KeyCode::Char('x')));
    assert!(!app.show_help);
    assert!(matches!(action, Action::None));
}

// ---------------------------------------------------------------------------
// Refresh
// ---------------------------------------------------------------------------

#[test]
fn r_returns_force_refresh() {
    let mut app = test_app();
    let action = handle_key(&mut app, key(KeyCode::Char('r')));
    assert!(matches!(action, Action::ForceRefresh));
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[test]
fn slash_enters_search_on_pipelines() {
    let mut app = test_app();
    app.view = View::Pipelines;
    let action = handle_key(&mut app, key(KeyCode::Char('/')));
    assert_eq!(app.search.mode, InputMode::Search);
    assert!(matches!(action, Action::None));
}

#[test]
fn slash_no_op_on_dashboard() {
    let mut app = test_app();
    app.view = View::Dashboard;
    let action = handle_key(&mut app, key(KeyCode::Char('/')));
    assert_eq!(app.search.mode, InputMode::Normal);
    assert!(matches!(action, Action::None));
}

#[test]
fn esc_exits_search() {
    let mut app = test_app();
    app.view = View::Pipelines;
    app.search.mode = InputMode::Search;
    app.search.query = "hello".into();

    let action = handle_key(&mut app, key(KeyCode::Esc));
    assert_eq!(app.search.mode, InputMode::Normal);
    assert!(app.search.query.is_empty());
    assert!(matches!(action, Action::None));
}

#[test]
fn typing_in_search_appends() {
    let mut app = test_app();
    app.view = View::Pipelines;
    app.search.mode = InputMode::Search;

    handle_key(&mut app, key(KeyCode::Char('a')));
    handle_key(&mut app, key(KeyCode::Char('b')));
    assert_eq!(app.search.query, "ab");
}

#[test]
fn backspace_in_search() {
    let mut app = test_app();
    app.view = View::Pipelines;
    app.search.mode = InputMode::Search;
    app.search.query = "abc".into();

    handle_key(&mut app, key(KeyCode::Backspace));
    assert_eq!(app.search.query, "ab");
}

#[test]
fn enter_commits_search() {
    let mut app = test_app();
    app.view = View::Pipelines;
    app.search.mode = InputMode::Search;
    app.search.query = "hello".into();

    let action = handle_key(&mut app, key(KeyCode::Enter));
    assert_eq!(app.search.mode, InputMode::Normal);
    assert_eq!(app.search.query, "hello"); // query preserved
    assert!(matches!(action, Action::None));
}

// ---------------------------------------------------------------------------
// Enter / drill-in
// ---------------------------------------------------------------------------

#[test]
fn enter_on_pipelines_fetches_history() {
    let mut app = test_app();
    app.view = View::Pipelines;
    app.data.definitions = vec![make_definition(1, "Pipeline 1", "\\")];
    app.rebuild_pipelines();
    // Navigate past the folder header to the first pipeline.
    app.pipelines.nav.set_index(1);

    let action = handle_key(&mut app, key(KeyCode::Enter));
    assert!(
        matches!(action, Action::FetchBuildHistory(1)),
        "expected FetchBuildHistory(1), got {action:?}"
    );
    assert_eq!(app.view, View::BuildHistory);
}

#[test]
fn enter_on_active_runs_fetches_timeline() {
    let mut app = test_app();
    app.view = View::ActiveRuns;
    app.data.active_builds = vec![make_build(42, BuildStatus::InProgress, None)];
    app.active_runs.rebuild(
        &app.data.active_builds,
        &app.filters.definition_ids,
        &app.search.query,
    );

    let action = handle_key(&mut app, key(KeyCode::Enter));
    assert!(
        matches!(action, Action::FetchTimeline(42)),
        "expected FetchTimeline(42), got {action:?}"
    );
    assert_eq!(app.view, View::LogViewer);
}

#[test]
fn enter_on_empty_list_is_none() {
    let mut app = test_app();
    app.view = View::Pipelines;
    // filtered_pipelines is empty (via pipelines.filtered)
    let action = handle_key(&mut app, key(KeyCode::Enter));
    assert!(matches!(action, Action::None));
}

// ---------------------------------------------------------------------------
// Confirm prompt
// ---------------------------------------------------------------------------

#[test]
fn confirm_y_executes_cancel() {
    let mut app = test_app();
    app.confirm_prompt = Some(ConfirmPrompt {
        message: "Cancel build #1?  [y/N]".into(),
        action: ConfirmAction::CancelBuild { build_id: 1 },
    });

    let action = handle_key(&mut app, key(KeyCode::Char('y')));
    assert!(app.confirm_prompt.is_none());
    assert!(
        matches!(action, Action::CancelBuild(1)),
        "expected CancelBuild(1), got {action:?}"
    );
}

#[test]
fn confirm_n_dismisses() {
    let mut app = test_app();
    app.confirm_prompt = Some(ConfirmPrompt {
        message: "Cancel?".into(),
        action: ConfirmAction::CancelBuild { build_id: 1 },
    });

    let action = handle_key(&mut app, key(KeyCode::Char('n')));
    assert!(app.confirm_prompt.is_none());
    assert!(matches!(action, Action::None));
}

#[test]
fn confirm_esc_dismisses() {
    let mut app = test_app();
    app.confirm_prompt = Some(ConfirmPrompt {
        message: "Cancel?".into(),
        action: ConfirmAction::CancelBuild { build_id: 1 },
    });

    let action = handle_key(&mut app, key(KeyCode::Esc));
    assert!(app.confirm_prompt.is_none());
    assert!(matches!(action, Action::None));
}

#[test]
fn confirm_blocks_other_keys() {
    let mut app = test_app();
    app.confirm_prompt = Some(ConfirmPrompt {
        message: "Cancel?".into(),
        action: ConfirmAction::CancelBuild { build_id: 1 },
    });

    // Random key should not dismiss the prompt or trigger quit
    let action = handle_key(&mut app, key(KeyCode::Char('q')));
    assert!(app.confirm_prompt.is_some());
    assert!(matches!(action, Action::None));
}

// ---------------------------------------------------------------------------
// Multi-select
// ---------------------------------------------------------------------------

#[test]
fn space_toggles_in_active_runs() {
    let mut app = test_app();
    app.view = View::ActiveRuns;
    app.data.active_builds = vec![make_build(10, BuildStatus::InProgress, None)];
    app.active_runs.rebuild(
        &app.data.active_builds,
        &app.filters.definition_ids,
        &app.search.query,
    );

    // Toggle on
    let action = handle_key(&mut app, key(KeyCode::Char(' ')));
    assert!(app.active_runs.selected.contains(&10));
    assert!(matches!(action, Action::None));

    // Toggle off
    let action = handle_key(&mut app, key(KeyCode::Char(' ')));
    assert!(!app.active_runs.selected.contains(&10));
    assert!(matches!(action, Action::None));
}

#[test]
fn space_noop_on_other_views() {
    let mut app = test_app();
    app.view = View::Dashboard;
    let action = handle_key(&mut app, key(KeyCode::Char(' ')));
    assert!(app.active_runs.selected.is_empty());
    assert!(matches!(action, Action::None));
}

// ---------------------------------------------------------------------------
// Cancel
// ---------------------------------------------------------------------------

#[test]
fn c_sets_confirm_on_active_runs() {
    let mut app = test_app();
    app.view = View::ActiveRuns;
    app.data.active_builds = vec![make_build(7, BuildStatus::InProgress, None)];
    app.active_runs.rebuild(
        &app.data.active_builds,
        &app.filters.definition_ids,
        &app.search.query,
    );

    let action = handle_key(&mut app, key(KeyCode::Char('c')));
    assert!(app.confirm_prompt.is_some());
    assert!(
        matches!(
            &app.confirm_prompt.as_ref().unwrap().action,
            ConfirmAction::CancelBuild { build_id: 7 }
        ),
        "expected CancelBuild for id 7"
    );
    assert!(matches!(action, Action::None));
}

// ---------------------------------------------------------------------------
// Open in browser
// ---------------------------------------------------------------------------

#[test]
fn o_opens_browser_on_dashboard() {
    let mut app = test_app();
    app.data.definitions = vec![make_definition(1, "Pipeline 1", "\\")];
    app.filters.pinned_definition_ids = vec![1];
    app.rebuild_dashboard();
    // Row 0 is "Pinned Pipelines" header; row 1 is the pinned pipeline.
    app.dashboard.nav.down();

    assert!(matches!(
        app.dashboard.rows.get(app.dashboard.nav.index()),
        Some(DashboardRow::PinnedPipeline { .. })
    ));

    let action = handle_key(&mut app, key(KeyCode::Char('o')));
    assert!(
        matches!(action, Action::OpenInBrowser(ref url) if url.contains("definitionId=1")),
        "expected OpenInBrowser with definitionId=1, got {action:?}"
    );
}

// ---------------------------------------------------------------------------
// Follow mode
// ---------------------------------------------------------------------------

#[test]
fn f_in_log_viewer_returns_follow_latest() {
    let mut app = test_app();
    let build = make_build(1, BuildStatus::InProgress, None);
    app.navigate_to_log_viewer(build);

    let action = handle_key(&mut app, key(KeyCode::Char('f')));
    assert!(matches!(action, Action::FollowLatest));
    assert!(app.log_viewer.is_following());
}

#[test]
fn f_outside_log_viewer_is_noop() {
    let mut app = test_app();
    app.view = View::Dashboard;
    let action = handle_key(&mut app, key(KeyCode::Char('f')));
    // 'f' has no binding outside LogViewer, falls through to _ => Action::None
    assert!(matches!(action, Action::None));
}

// ---------------------------------------------------------------------------
// Navigation keys
// ---------------------------------------------------------------------------

#[test]
fn arrow_keys_navigate_list() {
    let mut app = test_app();
    app.view = View::Pipelines;
    app.data.definitions = vec![
        make_definition(1, "Pipeline 1", "\\"),
        make_definition(2, "Pipeline 2", "\\"),
        make_definition(3, "Pipeline 3", "\\"),
    ];
    app.rebuild_pipelines();

    // Rows: folder header (0) + 3 pipelines (1,2,3).
    handle_key(&mut app, key(KeyCode::Down));
    assert_eq!(app.pipelines.nav.index(), 1);

    handle_key(&mut app, key(KeyCode::Down));
    assert_eq!(app.pipelines.nav.index(), 2);

    handle_key(&mut app, key(KeyCode::Up));
    assert_eq!(app.pipelines.nav.index(), 1);
}

#[test]
fn home_and_end_keys() {
    let mut app = test_app();
    app.view = View::Pipelines;
    app.data.definitions = vec![
        make_definition(1, "Pipeline 1", "\\"),
        make_definition(2, "Pipeline 2", "\\"),
        make_definition(3, "Pipeline 3", "\\"),
    ];
    app.rebuild_pipelines();

    // Rows: folder header (0) + 3 pipelines (1,2,3) = 4 rows.
    handle_key(&mut app, key(KeyCode::End));
    assert_eq!(app.pipelines.nav.index(), 3);

    handle_key(&mut app, key(KeyCode::Home));
    assert_eq!(app.pipelines.nav.index(), 0);
}

// ---------------------------------------------------------------------------
// Tab switching clears search
// ---------------------------------------------------------------------------

#[test]
fn tab_switching_clears_search_query() {
    let mut app = test_app();
    app.view = View::Pipelines;
    app.search.query = "hello".into();

    handle_key(&mut app, key(KeyCode::Char('1')));
    assert!(app.search.query.is_empty());
    assert_eq!(app.view, View::Dashboard);
}

// ---------------------------------------------------------------------------
// Slash also works on ActiveRuns
// ---------------------------------------------------------------------------

#[test]
fn slash_enters_search_on_active_runs() {
    let mut app = test_app();
    app.view = View::ActiveRuns;
    let action = handle_key(&mut app, key(KeyCode::Char('/')));
    assert_eq!(app.search.mode, InputMode::Search);
    assert!(matches!(action, Action::None));
}

// ---------------------------------------------------------------------------
// Esc goes back from views
// ---------------------------------------------------------------------------

#[test]
fn esc_goes_back_from_build_history() {
    let mut app = test_app();
    app.view = View::BuildHistory;
    app.build_history.return_to = Some(View::Pipelines);

    let action = handle_key(&mut app, key(KeyCode::Char('q')));
    assert_eq!(app.view, View::Pipelines);
    assert!(matches!(action, Action::None));
}

// ---------------------------------------------------------------------------
// Reload on connection change
// ---------------------------------------------------------------------------

#[test]
fn settings_save_no_reload_when_connection_unchanged() {
    let mut app = test_app();
    app.open_settings();
    assert!(app.show_settings);

    // Save without changing org/project — should not reload
    let action = handle_key(
        &mut app,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
    );
    assert!(!app.reload_requested);
    assert!(matches!(action, Action::None));
}

// ---------------------------------------------------------------------------
// Pull Requests view
// ---------------------------------------------------------------------------

#[test]
fn key_4_switches_to_pull_requests() {
    let mut app = test_app();
    let action = handle_key(&mut app, key(KeyCode::Char('4')));
    assert_eq!(app.view, View::PullRequests);
    assert!(matches!(action, Action::FetchPullRequests));
}

#[test]
fn tab_cycles_pr_mode() {
    use azure_pipelines_cli::components::pull_requests::PrViewMode;

    let mut app = test_app();
    app.view = View::PullRequests;
    assert_eq!(app.pull_requests.mode, PrViewMode::CreatedByMe);

    let action = handle_key(&mut app, key(KeyCode::Tab));
    assert_eq!(app.pull_requests.mode, PrViewMode::AssignedToMe);
    assert!(matches!(action, Action::FetchPullRequests));

    let action = handle_key(&mut app, key(KeyCode::Tab));
    assert_eq!(app.pull_requests.mode, PrViewMode::AllActive);
    assert!(matches!(action, Action::FetchPullRequests));

    let action = handle_key(&mut app, key(KeyCode::Tab));
    assert_eq!(app.pull_requests.mode, PrViewMode::CreatedByMe);
    assert!(matches!(action, Action::FetchPullRequests));
}

#[test]
fn slash_enters_search_on_pull_requests() {
    let mut app = test_app();
    app.view = View::PullRequests;

    handle_key(&mut app, key(KeyCode::Char('/')));
    assert_eq!(app.search.mode, InputMode::Search);
}

#[test]
fn pr_search_filters_list() {
    let mut app = test_app();
    app.view = View::PullRequests;

    // Populate PRs.
    app.pull_requests.set_data(
        vec![
            make_pull_request(1, "Add feature", "active", "frontend"),
            make_pull_request(2, "Fix bug", "active", "backend"),
        ],
        "",
    );
    assert_eq!(app.pull_requests.filtered.len(), 2);

    // Enter search mode and type "bug".
    handle_key(&mut app, key(KeyCode::Char('/')));
    handle_key(&mut app, key(KeyCode::Char('b')));
    handle_key(&mut app, key(KeyCode::Char('u')));
    handle_key(&mut app, key(KeyCode::Char('g')));
    assert_eq!(app.pull_requests.filtered.len(), 1);
    assert_eq!(app.pull_requests.filtered[0].pull_request_id, 2);
}

#[test]
fn q_on_pull_requests_goes_to_dashboard() {
    let mut app = test_app();
    app.view = View::PullRequests;

    handle_key(&mut app, key(KeyCode::Char('q')));
    assert_eq!(app.view, View::Dashboard);
}

#[test]
fn esc_on_pull_requests_goes_to_dashboard() {
    let mut app = test_app();
    app.view = View::PullRequests;

    handle_key(&mut app, key(KeyCode::Esc));
    assert_eq!(app.view, View::Dashboard);
}

#[test]
fn tab_switching_clears_search_on_pr_switch() {
    let mut app = test_app();
    app.view = View::Pipelines;
    app.search.query = "old query".to_string();

    handle_key(&mut app, key(KeyCode::Char('4')));
    assert_eq!(app.view, View::PullRequests);
    assert!(app.search.query.is_empty());
}

#[test]
fn o_opens_browser_on_pull_requests() {
    let mut app = test_app();
    app.view = View::PullRequests;
    app.pull_requests.set_data(
        vec![make_pull_request(42, "Test PR", "active", "my-repo")],
        "",
    );

    let action = handle_key(&mut app, key(KeyCode::Char('o')));
    assert!(
        matches!(action, Action::OpenInBrowser(url) if url.contains("my-repo") && url.contains("42"))
    );
}

// ---------------------------------------------------------------------------
// Pull Request Detail view
// ---------------------------------------------------------------------------

#[test]
fn enter_on_pr_list_drills_into_detail() {
    let mut app = test_app();
    app.view = View::PullRequests;
    app.pull_requests.set_data(
        vec![make_pull_request(42, "Test PR", "active", "my-repo")],
        "",
    );

    let action = handle_key(&mut app, key(KeyCode::Enter));
    assert_eq!(app.view, View::PullRequestDetail);
    assert!(app.pull_request_detail.loading);
    assert!(matches!(
        action,
        Action::FetchPullRequestDetail { pr_id: 42, .. }
    ));
}

#[test]
fn right_on_pr_list_drills_into_detail() {
    let mut app = test_app();
    app.view = View::PullRequests;
    app.pull_requests.set_data(
        vec![make_pull_request(99, "Another PR", "active", "backend")],
        "",
    );

    let action = handle_key(&mut app, key(KeyCode::Right));
    assert_eq!(app.view, View::PullRequestDetail);
    assert!(matches!(
        action,
        Action::FetchPullRequestDetail { pr_id: 99, .. }
    ));
}

#[test]
fn q_on_pr_detail_goes_back_to_pr_list() {
    let mut app = test_app();
    app.view = View::PullRequests;
    app.pull_requests.set_data(
        vec![make_pull_request(42, "Test PR", "active", "my-repo")],
        "",
    );
    handle_key(&mut app, key(KeyCode::Enter));
    assert_eq!(app.view, View::PullRequestDetail);

    handle_key(&mut app, key(KeyCode::Char('q')));
    assert_eq!(app.view, View::PullRequests);
}

#[test]
fn esc_on_pr_detail_goes_back_to_pr_list() {
    let mut app = test_app();
    app.view = View::PullRequests;
    app.pull_requests.set_data(
        vec![make_pull_request(42, "Test PR", "active", "my-repo")],
        "",
    );
    handle_key(&mut app, key(KeyCode::Enter));
    assert_eq!(app.view, View::PullRequestDetail);

    handle_key(&mut app, key(KeyCode::Esc));
    assert_eq!(app.view, View::PullRequests);
}

#[test]
fn left_on_pr_detail_goes_back() {
    let mut app = test_app();
    app.view = View::PullRequests;
    app.pull_requests.set_data(
        vec![make_pull_request(42, "Test PR", "active", "my-repo")],
        "",
    );
    handle_key(&mut app, key(KeyCode::Enter));
    assert_eq!(app.view, View::PullRequestDetail);

    handle_key(&mut app, key(KeyCode::Left));
    assert_eq!(app.view, View::PullRequests);
}

#[test]
fn enter_on_empty_pr_list_is_noop() {
    let mut app = test_app();
    app.view = View::PullRequests;
    // No PRs loaded.
    let action = handle_key(&mut app, key(KeyCode::Enter));
    assert_eq!(app.view, View::PullRequests);
    assert!(matches!(action, Action::None));
}
