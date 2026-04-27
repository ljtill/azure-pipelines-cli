//! Snapshot tests for core view renderers using ratatui's `TestBackend`.
//!
//! These tests pin layout, column widths, truncation, and status icon rendering
//! for the primary views. Snapshots are inline `String` literals compared via
//! `assert_eq!` — no external snapshot crate is used.
//!
//! To regenerate a snapshot after an intentional layout change, set the
//! `UPDATE_SNAPSHOTS` env var and run with `-- --nocapture`, then copy the
//! printed output into the corresponding `expected` literal.

use std::path::PathBuf;

use azure_devops_cli::client::models::{
    AssignedToField, BuildDefinitionRef, BuildResult, BuildStatus, IdentityRef, WorkItem,
    WorkItemFields,
};
use azure_devops_cli::shared::availability::Availability;
use azure_devops_cli::shared::log_buffer::DEFAULT_CAPACITY;
use azure_devops_cli::state::{App, PinnedWorkItemsState, View};
use azure_devops_cli::test_helpers::{
    make_app, make_build, make_config, make_definition, make_pull_request, make_simple_timeline,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

// --- Helpers ---

/// Converts a `TestBackend` buffer to a multi-line string. Each line is
/// right-trimmed of spaces so snapshots stay compact. Styling is ignored;
/// only rendered glyphs are captured.
fn buffer_to_string(buf: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        let mut line = String::new();
        for x in 0..buf.area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        out.push_str(line.trim_end_matches(' '));
        out.push('\n');
    }
    out
}

/// Compares `rendered` to `expected`. When `UPDATE_SNAPSHOTS` is set, the
/// actual rendered output is always printed so the expected literal can be
/// regenerated.
#[track_caller]
fn assert_snapshot(name: &str, rendered: &str, expected: &str) {
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        println!("=== {name} ===\n{rendered}=== end ===");
    }
    assert_eq!(rendered, expected, "snapshot {name} mismatch");
}

/// Builds a `WorkItem` fixture.
fn make_work_item(id: u32, title: &str, state: &str, assignee: Option<&str>) -> WorkItem {
    WorkItem {
        id,
        rev: Some(1),
        fields: WorkItemFields {
            title: title.to_string(),
            work_item_type: "User Story".to_string(),
            state: Some(state.to_string()),
            assigned_to: assignee.map(|name| {
                AssignedToField::Identity(IdentityRef {
                    id: None,
                    unique_name: None,
                    descriptor: None,
                    display_name: name.to_string(),
                })
            }),
            ..WorkItemFields::default()
        },
        relations: vec![],
        url: None,
    }
}

/// Seeds a dashboard-focused `App` with two pinned pipelines and two pinned
/// work items so the dashboard snapshot exercises both list sections.
fn make_dashboard_app() -> App {
    let config = make_config();
    let mut app = App::new(
        &config.devops.connection.organization,
        &config.devops.connection.project,
        &config,
        PathBuf::from("/tmp/test-config.toml"),
    );

    let def1 = make_definition(1, "CI Pipeline", "\\");
    let def2 = make_definition(2, "Deploy Pipeline", "\\Infra");
    let def3 = make_definition(3, "Infra Lint", "\\Infra");
    app.core.data.definitions = vec![def1, def2, def3];

    let mut b1 = make_build(100, BuildStatus::Completed, Some(BuildResult::Succeeded));
    b1.definition = BuildDefinitionRef {
        id: 1,
        name: "CI Pipeline".to_string(),
    };
    let mut b2 = make_build(101, BuildStatus::InProgress, None);
    b2.definition = BuildDefinitionRef {
        id: 2,
        name: "Deploy Pipeline".to_string(),
    };
    let mut b3 = make_build(102, BuildStatus::Completed, Some(BuildResult::Failed));
    b3.definition = BuildDefinitionRef {
        id: 3,
        name: "Infra Lint".to_string(),
    };
    app.core.data.recent_builds = vec![b1.clone(), b2.clone(), b3.clone()];
    app.core.data.latest_builds_by_def.insert(1, b1);
    app.core.data.latest_builds_by_def.insert(2, b2);
    app.core.data.latest_builds_by_def.insert(3, b3);
    app.core.availability.definitions = Availability::fresh(app.core.data.definitions.clone());
    app.core.availability.recent_builds = Availability::fresh(app.core.data.recent_builds.clone());
    app.core.availability.pending_approvals =
        Availability::fresh(app.core.data.pending_approvals.clone());

    app.core.filters.pinned_definition_ids = vec![1, 2];
    app.pinned_work_items = PinnedWorkItemsState::Ready(vec![
        make_work_item(501, "Investigate flaky test", "Active", Some("Alice")),
        make_work_item(
            502,
            "Document pipeline conventions",
            "New",
            Some("Bob Smith"),
        ),
    ]);

    app.rebuild_dashboard();
    app.rebuild_pipelines();
    app
}

// --- Dashboard: narrow (80x24) ---

#[test]
fn snapshot_dashboard_narrow() {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let app = make_dashboard_app();
    terminal
        .draw(|f| app.dashboard.draw_with_app(f, &app, f.area()))
        .unwrap();
    let rendered = buffer_to_string(terminal.backend().buffer());
    let expected = "\
┌ Dashboard ───────────────────────────────────────────────────────────────────┐
│ Pipelines (Pinned) ───────────────────────────────────────────────────────── │
│✓   Succeeded   CI Pipeline         #100          main        Unknown         │
│●   Running     Deploy Pipeline     #101          main        Unknown         │
│                                                                              │
│                                                                              │
│                                                                              │
│ Work Items (Pinned) ──────────────────────────────────────────────────────── │
│#501     User Story  Investigate flaky test Active      Alice                 │
│#502     User Story  Document pipeline conv…New         Bob Smith             │
│                                                                              │
│                                                                              │
│                                                                              │
│ Pull Requests (Active) ───────────────────────────────────────────────────── │
│    Loading pull requests...                                                  │
│                                                                              │
│                                                                              │
│                                                                              │
│ Work Items (Active) ──────────────────────────────────────────────────────── │
│    Loading work items...                                                     │
│                                                                              │
│                                                                              │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
";
    assert_snapshot("dashboard_narrow", &rendered, expected);
}

// --- Dashboard: wide (160x40), first 10 rows only ---

#[test]
fn snapshot_dashboard_wide_header() {
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    let app = make_dashboard_app();
    terminal
        .draw(|f| app.dashboard.draw_with_app(f, &app, f.area()))
        .unwrap();
    let rendered = buffer_to_string(terminal.backend().buffer());
    // Compare only the top 10 rows — the flex columns at 160x40 are the signal.
    let head: String = rendered.lines().take(10).collect::<Vec<_>>().join("\n") + "\n";
    let expected = "\
┌ Dashboard ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ Pipelines (Pinned) ───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────── │
│✓   Succeeded   CI Pipeline                                                  #100          main                          Unknown                              │
│●   Running     Deploy Pipeline                                              #101          main                          Unknown                        queued│
│                                                                                                                                                              │
│                                                                                                                                                              │
│                                                                                                                                                              │
│                                                                                                                                                              │
│                                                                                                                                                              │
│                                                                                                                                                              │
";
    assert_snapshot("dashboard_wide_header", &head, expected);
}

// --- Build history (80x24) with mixed statuses and a non-ASCII name ---

#[test]
fn snapshot_build_history_mixed_statuses() {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut app = make_app();
    let def = make_definition(1, "CI Pipeline", "\\");
    app.navigate_to_build_history(def);

    let mut b1 = make_build(1001, BuildStatus::Completed, Some(BuildResult::Succeeded));
    b1.build_number = "20240101.1".to_string();
    b1.definition = BuildDefinitionRef {
        id: 1,
        name: "CI Pipeline".to_string(),
    };

    // Exercise the W2.2 unicode-width handling via a CJK + emoji build name.
    let mut b2 = make_build(1002, BuildStatus::Completed, Some(BuildResult::Failed));
    b2.build_number = "デプロイ 🚀".to_string();
    b2.definition = BuildDefinitionRef {
        id: 1,
        name: "CI Pipeline".to_string(),
    };

    let mut b3 = make_build(1003, BuildStatus::InProgress, None);
    b3.build_number = "20240101.3".to_string();
    b3.definition = BuildDefinitionRef {
        id: 1,
        name: "CI Pipeline".to_string(),
    };

    let mut b4 = make_build(1004, BuildStatus::Completed, Some(BuildResult::Canceled));
    b4.build_number = "20240101.4".to_string();
    b4.definition = BuildDefinitionRef {
        id: 1,
        name: "CI Pipeline".to_string(),
    };

    app.build_history.builds = vec![b1, b2, b3, b4];
    app.build_history.nav.set_len(4);
    app.view = View::BuildHistory;

    terminal
        .draw(|f| app.build_history.draw_with_app(f, &app, f.area()))
        .unwrap();
    let rendered = buffer_to_string(terminal.backend().buffer());
    let expected = "\
┌ Build History ───────────────────────────────────────────────────────────────┐
│ CI Pipeline  ·  4 builds  ·  0 selected                                      │
│      Status      Build           Branch          Requestor            Elapsed│
│   ✓ Succeeded   #20240101.1     main            Unknown                      │
│   ✗ Failed      #デ プ ロ イ  🚀     main            Unknown                      │
│   ● Running     #20240101.3     main            Unknown               queued │
│   ⊘ Canceled    #20240101.4     main            Unknown                      │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
";
    assert_snapshot("build_history_mixed", &rendered, expected);
}

// --- Pipelines list (80x24) ---

#[test]
fn snapshot_pipelines_list() {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let app = make_app();
    terminal
        .draw(|f| app.pipelines.draw_with_app(f, &app, f.area()))
        .unwrap();
    let rendered = buffer_to_string(terminal.backend().buffer());
    let expected = "\
┌ Pipelines ───────────────────────────────────────────────────────────────────┐
│3 pipelines  ·  0 selected                                                    │
│      Status      Pipeline            Build         Branch      Requestor     │
│ ▾ Infra                                                                      │
│    ● Running     Deploy Pipeline     #101          main        Unknown       │
│    ✗ Failed      Infra Lint          #102          main        Unknown       │
│  ✓ Succeeded   CI Pipeline         #100          main        Unknown         │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
";
    assert_snapshot("pipelines_list", &rendered, expected);
}

// --- Pull requests list (80x24) ---

#[test]
fn snapshot_pull_requests_list() {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut app = make_app();
    app.view = View::PullRequestsAllActive;

    let mut pr1 = make_pull_request(42, "Add metrics endpoint", "active", "azure-devops-cli");
    pr1.created_by = Some(IdentityRef {
        id: None,
        unique_name: None,
        descriptor: None,
        display_name: "Alice".to_string(),
    });
    let mut pr2 = make_pull_request(
        43,
        "Refactor build history column layout",
        "active",
        "azure-devops-cli",
    );
    pr2.is_draft = true;
    let pr3 = make_pull_request(
        44,
        "Fix unicode width in log tail",
        "active",
        "some-other-repo",
    );

    app.pull_requests.set_data(vec![pr1, pr2, pr3], "");

    terminal
        .draw(|f| app.pull_requests.draw_with_app(f, &app, f.area()))
        .unwrap();
    let rendered = buffer_to_string(terminal.backend().buffer());
    let expected = "\
┌ Pull Requests ───────────────────────────────────────────────────────────────┐
│  ·  3 shown                                                                  │
│    Title                         Repo        Author      Target        Votes │
│●   #42 Add metrics endpoint      azure-devop…Alice       main                │
│●   #44 Fix unicode width in log …some-other-…Test User   main                │
│◌   #43 Refactor build history co…azure-devop…Test User   main                │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
";
    assert_snapshot("pull_requests_list", &rendered, expected);
}

// --- Log viewer in follow mode with a collapsed stage (80x24) ---

#[test]
fn snapshot_log_viewer_follow_mode() {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut app = make_app();
    let build = {
        let mut b = make_build(2001, BuildStatus::InProgress, None);
        b.definition = BuildDefinitionRef {
            id: 1,
            name: "CI Pipeline".to_string(),
        };
        b.build_number = "20240101.7".to_string();
        b
    };
    app.navigate_to_log_viewer(build);
    app.log_viewer.set_build_timeline(make_simple_timeline());
    app.log_viewer.rebuild_timeline_rows();
    app.log_viewer.enter_follow_mode();
    app.log_viewer
        .set_followed("Build Solution".to_string(), 11);
    // Expand the second stage so the snapshot shows both a collapsed (default)
    // stage and an expanded stage side by side.
    let second_stage_id = app
        .log_viewer
        .timeline_rows()
        .iter()
        .filter_map(|row| match row {
            azure_devops_cli::state::TimelineRow::Stage { id, .. } => Some(id.clone()),
            _ => None,
        })
        .nth(1);
    if let Some(id) = second_stage_id {
        app.log_viewer.toggle_stage(&id);
        app.log_viewer.rebuild_timeline_rows();
    }

    terminal
        .draw(|f| {
            azure_devops_cli::components::log_viewer::draw_log_viewer(f, &mut app, f.area());
        })
        .unwrap();
    let rendered = buffer_to_string(terminal.backend().buffer());
    let expected = "\
┌ Log Viewer ──────────────────────────────────────────────────────────────────┐
│ CI Pipeline #20240101.7  ·  Follow mode                                      │
│┌ Pipeline Stages ────────┐┌ Log Output — FOLLOW: Build Solution ────────────┐│
││▸ ✓ Build                ││ Select a task and press Enter to view its log   ││
││▾ ✓ Deploy               ││                                                 ││
││  ▸ ✓ Deploy Job         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
││                         ││                                                 ││
│└─────────────────────────┘└─────────────────────────────────────────────────┘│
└──────────────────────────────────────────────────────────────────────────────┘
";
    assert_snapshot("log_viewer_follow", &rendered, expected);
}

#[test]
fn log_viewer_large_log_renders_visible_tail() {
    let mut terminal = Terminal::new(TestBackend::new(80, 18)).unwrap();
    let mut app = make_app();
    app.navigate_to_log_viewer(make_build(
        2002,
        BuildStatus::Completed,
        Some(BuildResult::Succeeded),
    ));
    let total_lines = DEFAULT_CAPACITY + 123;
    let log = (0..total_lines)
        .map(|i| format!("line-{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.log_viewer.set_log_content(&log);

    terminal
        .draw(|f| {
            azure_devops_cli::components::log_viewer::draw_log_viewer(f, &mut app, f.area());
        })
        .unwrap();

    let rendered = buffer_to_string(terminal.backend().buffer());
    assert_eq!(app.log_viewer.log_content().len(), DEFAULT_CAPACITY);
    assert_eq!(app.log_viewer.log_content().dropped(), 123);
    let last_line = format!("line-{}", total_lines - 1);
    assert!(rendered.contains(&last_line));
    assert!(!rendered.contains("line-0"));
    assert!(rendered.matches("line-").count() <= 14);
}

#[test]
fn log_viewer_large_log_manual_scroll_renders_visible_window() {
    let mut terminal = Terminal::new(TestBackend::new(80, 18)).unwrap();
    let mut app = make_app();
    app.navigate_to_log_viewer(make_build(
        2003,
        BuildStatus::Completed,
        Some(BuildResult::Succeeded),
    ));
    let log = (0..10_000)
        .map(|i| format!("line-{i:04}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.log_viewer.set_log_content(&log);
    app.log_viewer.set_log_auto_scroll(false);
    app.log_viewer.set_log_scroll_offset(5_000);

    terminal
        .draw(|f| {
            azure_devops_cli::components::log_viewer::draw_log_viewer(f, &mut app, f.area());
        })
        .unwrap();

    let rendered = buffer_to_string(terminal.backend().buffer());
    assert!(rendered.contains("line-5000"));
    assert!(!rendered.contains("line-0000"));
    assert!(!rendered.contains("line-9999"));
    assert!(rendered.matches("line-").count() <= 14);
}

#[test]
fn build_history_large_list_renders_visible_tail() {
    let mut terminal = Terminal::new(TestBackend::new(80, 18)).unwrap();
    let mut app = make_app();
    let definition = make_definition(9, "Scale Pipeline", "\\");
    app.navigate_to_build_history(definition);

    let total_builds = 5_000;
    app.build_history.builds = (0..total_builds)
        .map(|i| {
            let mut build = make_build(
                3_000 + i as u32,
                BuildStatus::Completed,
                Some(BuildResult::Succeeded),
            );
            build.build_number = format!("scale-{i:04}");
            build.definition = BuildDefinitionRef {
                id: 9,
                name: "Scale Pipeline".to_string(),
            };
            build
        })
        .collect();
    app.build_history.nav.set_len(total_builds);
    app.build_history.nav.set_index(total_builds - 1);

    terminal
        .draw(|f| app.build_history.draw_with_app(f, &app, f.area()))
        .unwrap();

    let rendered = buffer_to_string(terminal.backend().buffer());
    assert!(rendered.contains("#scale-4999"));
    assert!(!rendered.contains("#scale-0000"));
    assert!(rendered.matches("#scale-").count() <= 15);
}

#[test]
fn pull_requests_large_list_renders_visible_tail() {
    let mut terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();
    let mut app = make_app();
    app.view = View::PullRequestsAllActive;

    let total_prs = 5_000;
    let prs = (0..total_prs)
        .map(|i| make_pull_request(i as u32, &format!("Scale PR {i:04}"), "active", "repo"))
        .collect();
    app.pull_requests.set_data(prs, "");
    app.pull_requests.nav.set_index(total_prs - 1);

    terminal
        .draw(|f| app.pull_requests.draw_with_app(f, &app, f.area()))
        .unwrap();

    let rendered = buffer_to_string(terminal.backend().buffer());
    assert!(rendered.contains("Scale PR 4999"));
    assert!(!rendered.contains("Scale PR 0000"));
    assert!(rendered.matches("Scale PR").count() <= 15);
}
