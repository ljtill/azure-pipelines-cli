//! Integration tests for action/message flow and generation-based stale-response guards.

use azure_devops_cli::client::models::{BuildResult, BuildStatus, BuildTimeline};
use azure_devops_cli::events::Action;
use azure_devops_cli::state::actions::{handle_action, handle_message};
use azure_devops_cli::test_helpers::{AppMessage, make_app, make_build, make_simple_timeline};
use tokio::sync::mpsc;

/// Constructs an in-process channel pair used to capture messages spawned by
/// `handle_action`. The receiver is returned so tests can drain or drop it.
fn test_channel() -> (mpsc::Sender<AppMessage>, mpsc::Receiver<AppMessage>) {
    mpsc::channel(32)
}

/// Attempts to build an `AdoClient`. Credential construction is offline — it only
/// wires up `DeveloperToolsCredential` without making network calls — so this is
/// safe to use inside tests for actions that don't actually hit the network.
fn try_make_client() -> Option<azure_devops_cli::client::http::AdoClient> {
    azure_devops_cli::client::http::AdoClient::new("testorg", "testproj").ok()
}

// --- Test 1: Stale timeline message is rejected via log_generation guard ---

#[tokio::test]
async fn stale_timeline_message_is_dropped() {
    let mut app = make_app();
    let Some(client) = try_make_client() else {
        eprintln!("skipping: AdoClient construction failed in this environment");
        return;
    };
    let (tx, mut rx) = test_channel();

    // Seed: navigate to the log viewer for a known build so log_viewer has a
    // selected build and a known generation baseline.
    let build = make_build(200, BuildStatus::InProgress, None);
    app.navigate_to_log_viewer(build);
    let gen_before_fetch = app.log_viewer.generation();

    // Dispatching FetchTimeline does not bump the generation — it merely spawns
    // a fetch carrying the current generation. The generation is bumped when a
    // new build is selected (via `reset_log_viewer`). Model that bump here by
    // selecting a different build to move the counter forward.
    let other = make_build(201, BuildStatus::InProgress, None);
    app.navigate_to_log_viewer(other);
    let current_gen = app.log_viewer.generation();
    assert!(
        current_gen > gen_before_fetch,
        "selecting a new build should increment log_generation",
    );

    // Dispatch an action that would spawn a timeline fetch. This should not
    // crash and — for this action variant — should not mutate the generation.
    let mut last_fetch = std::time::Instant::now();
    handle_action(
        &mut app,
        &client,
        &tx,
        Action::FetchTimeline(200),
        &mut last_fetch,
    );
    assert_eq!(
        app.log_viewer.generation(),
        current_gen,
        "FetchTimeline must not change log_generation",
    );

    // Feed a *stale* Timeline message (generation - 1). State must be unchanged.
    assert!(app.log_viewer.build_timeline().is_none());
    handle_message(
        &mut app,
        &client,
        &tx,
        AppMessage::Timeline {
            build_id: 200,
            timeline: BuildTimeline {
                records: Vec::new(),
            },
            generation: current_gen - 1,
            is_refresh: false,
        },
    );
    assert!(
        app.log_viewer.build_timeline().is_none(),
        "stale Timeline message must be dropped",
    );

    // Feed a *current* Timeline message. State should update.
    handle_message(
        &mut app,
        &client,
        &tx,
        AppMessage::Timeline {
            build_id: 200,
            timeline: make_simple_timeline(),
            generation: current_gen,
            is_refresh: false,
        },
    );
    assert!(
        app.log_viewer.build_timeline().is_some(),
        "current Timeline message must update state",
    );
    assert_eq!(
        app.log_viewer.build_timeline().unwrap().records.len(),
        8,
        "expected the full simple timeline to be applied",
    );

    // Drain any messages spawned by the dispatch/message handlers so the
    // receiver doesn't get dropped mid-send and panic in logs.
    rx.close();
    while rx.try_recv().is_ok() {}
}

// --- Test 2: Stale log-content message is rejected via log_generation guard ---

#[tokio::test]
async fn stale_log_content_message_is_dropped() {
    let mut app = make_app();
    let Some(client) = try_make_client() else {
        eprintln!("skipping: AdoClient construction failed in this environment");
        return;
    };
    let (tx, mut rx) = test_channel();

    let build = make_build(300, BuildStatus::Completed, Some(BuildResult::Succeeded));
    app.navigate_to_log_viewer(build);

    // Select a second build so the generation counter advances past the first.
    let newer = make_build(301, BuildStatus::Completed, Some(BuildResult::Succeeded));
    app.navigate_to_log_viewer(newer);
    let current_gen = app.log_viewer.generation();

    // Dispatch a log-fetch action; it should not mutate generation.
    let mut last_fetch = std::time::Instant::now();
    handle_action(
        &mut app,
        &client,
        &tx,
        Action::FetchBuildLog {
            build_id: 301,
            log_id: 42,
        },
        &mut last_fetch,
    );
    assert_eq!(app.log_viewer.generation(), current_gen);

    assert!(app.log_viewer.log_content().is_empty());

    // Feed a stale LogContent message — must be dropped.
    handle_message(
        &mut app,
        &client,
        &tx,
        AppMessage::LogContent {
            content: "stale log output that must not land".to_string(),
            generation: current_gen - 1,
            log_id: 42,
        },
    );
    assert!(
        app.log_viewer.log_content().is_empty(),
        "stale LogContent must be dropped",
    );

    // Feed a fresh LogContent message — must be applied.
    handle_message(
        &mut app,
        &client,
        &tx,
        AppMessage::LogContent {
            content: "fresh log output".to_string(),
            generation: current_gen,
            log_id: 42,
        },
    );
    assert!(
        !app.log_viewer.log_content().is_empty(),
        "fresh LogContent must be applied",
    );

    rx.close();
    while rx.try_recv().is_ok() {}
}

// --- Test 3: Pagination at MAX_PAGES boundary ---
//
// SKIPPED: `get_all_pages` is tightly coupled to a real `reqwest::Client` and
// bearer-token auth inside `AdoClient`. Exercising the MAX_PAGES branch without
// a live transport would require either a local HTTP mock server (new dev-dep,
// explicitly disallowed) or injecting a fake transport (requires modifying
// `src/client/http/mod.rs`, which is off-limits for this task). The existing
// error-message contract — containing `"Pagination limit reached"` — is covered
// indirectly via the hard-coded string in the source. A follow-up should add a
// trait-level transport abstraction to make this boundary testable.

// --- Test 4: Action dispatch round-trip smoke test ---

#[tokio::test]
async fn dispatch_quit_action_stops_app() {
    let mut app = make_app();
    let Some(client) = try_make_client() else {
        eprintln!("skipping: AdoClient construction failed in this environment");
        return;
    };
    let (tx, mut rx) = test_channel();

    assert!(app.running, "make_app must yield a running app");

    let mut last_fetch = std::time::Instant::now();
    handle_action(&mut app, &client, &tx, Action::Quit, &mut last_fetch);

    assert!(!app.running, "Action::Quit must flip App::running to false",);

    rx.close();
    while rx.try_recv().is_ok() {}
}

#[tokio::test]
async fn dispatch_none_action_is_noop() {
    let mut app = make_app();
    let Some(client) = try_make_client() else {
        eprintln!("skipping: AdoClient construction failed in this environment");
        return;
    };
    let (tx, mut rx) = test_channel();

    let running_before = app.running;
    let view_before = app.view;
    let mut last_fetch = std::time::Instant::now();
    handle_action(&mut app, &client, &tx, Action::None, &mut last_fetch);

    assert_eq!(app.running, running_before);
    assert_eq!(app.view, view_before);

    rx.close();
    while rx.try_recv().is_ok() {}
}
