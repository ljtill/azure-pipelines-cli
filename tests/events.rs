use azure_pipelines_cli::api::models::*;
use azure_pipelines_cli::app::{App, ConfirmAction, ConfirmPrompt, DashboardRow, InputMode, View};
use azure_pipelines_cli::events::{Action, handle_key};
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
    App::new("o", "p", &make_config())
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
    app.definitions = vec![make_definition(1, "Pipeline 1", "\\")];
    let action = handle_key(&mut app, key(KeyCode::Char('2')));
    assert_eq!(app.view, View::Pipelines);
    assert!(!app.pipelines.filtered.is_empty());
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
// Quit behavior
// ---------------------------------------------------------------------------

#[test]
fn q_quits_from_dashboard() {
    let mut app = test_app();
    let action = handle_key(&mut app, key(KeyCode::Char('q')));
    assert!(matches!(action, Action::Quit));
}

#[test]
fn q_quits_from_pipelines() {
    let mut app = test_app();
    app.view = View::Pipelines;
    let action = handle_key(&mut app, key(KeyCode::Char('q')));
    assert!(matches!(action, Action::Quit));
}

#[test]
fn q_goes_back_from_build_history() {
    let mut app = test_app();
    app.view = View::BuildHistory;
    app.build_history.return_to = Some(View::Dashboard);
    let action = handle_key(&mut app, key(KeyCode::Char('q')));
    assert_eq!(app.view, View::Dashboard);
    assert!(matches!(action, Action::None));
}

#[test]
fn q_goes_back_from_log_viewer() {
    let mut app = test_app();
    let build = make_build(1, BuildStatus::InProgress, None);
    app.navigate_to_log_viewer(build);
    assert_eq!(app.view, View::LogViewer);

    let action = handle_key(&mut app, key(KeyCode::Char('q')));
    assert_ne!(app.view, View::LogViewer);
    assert!(matches!(action, Action::None));
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
    app.definitions = vec![make_definition(1, "Pipeline 1", "\\")];
    app.pipelines.rebuild(
        &app.definitions,
        &app.filter_folders,
        &app.filter_definition_ids,
        &app.search.query,
    );
    app.pipelines.nav.set_len(app.pipelines.filtered.len());

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
    app.active_builds = vec![make_build(42, BuildStatus::InProgress, None)];
    app.rebuild_filtered_active_builds();

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
    app.active_builds = vec![make_build(10, BuildStatus::InProgress, None)];
    app.rebuild_filtered_active_builds();

    // Toggle on
    let action = handle_key(&mut app, key(KeyCode::Char(' ')));
    assert!(app.selected_builds.contains(&10));
    assert!(matches!(action, Action::None));

    // Toggle off
    let action = handle_key(&mut app, key(KeyCode::Char(' ')));
    assert!(!app.selected_builds.contains(&10));
    assert!(matches!(action, Action::None));
}

#[test]
fn space_noop_on_other_views() {
    let mut app = test_app();
    app.view = View::Dashboard;
    let action = handle_key(&mut app, key(KeyCode::Char(' ')));
    assert!(app.selected_builds.is_empty());
    assert!(matches!(action, Action::None));
}

// ---------------------------------------------------------------------------
// Cancel
// ---------------------------------------------------------------------------

#[test]
fn c_sets_confirm_on_active_runs() {
    let mut app = test_app();
    app.view = View::ActiveRuns;
    app.active_builds = vec![make_build(7, BuildStatus::InProgress, None)];
    app.rebuild_filtered_active_builds();

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
    app.definitions = vec![make_definition(1, "Pipeline 1", "\\")];
    app.rebuild_dashboard_rows();
    app.dashboard_nav.set_len(app.dashboard_rows.len());
    // Row 0 is a folder header; move to row 1 which is the pipeline
    app.dashboard_nav.down();

    // Verify we are on a Pipeline row
    assert!(matches!(
        app.dashboard_rows.get(app.dashboard_nav.index()),
        Some(DashboardRow::Pipeline { .. })
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
    app.definitions = vec![
        make_definition(1, "Pipeline 1", "\\"),
        make_definition(2, "Pipeline 2", "\\"),
        make_definition(3, "Pipeline 3", "\\"),
    ];
    app.pipelines.rebuild(
        &app.definitions,
        &app.filter_folders,
        &app.filter_definition_ids,
        &app.search.query,
    );
    app.pipelines.nav.set_len(app.pipelines.filtered.len());

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
    app.definitions = vec![
        make_definition(1, "Pipeline 1", "\\"),
        make_definition(2, "Pipeline 2", "\\"),
        make_definition(3, "Pipeline 3", "\\"),
    ];
    app.pipelines.rebuild(
        &app.definitions,
        &app.filter_folders,
        &app.filter_definition_ids,
        &app.search.query,
    );
    app.pipelines.nav.set_len(app.pipelines.filtered.len());

    handle_key(&mut app, key(KeyCode::End));
    assert_eq!(app.pipelines.nav.index(), 2);

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

    let action = handle_key(&mut app, key(KeyCode::Esc));
    assert_eq!(app.view, View::Pipelines);
    assert!(matches!(action, Action::None));
}
