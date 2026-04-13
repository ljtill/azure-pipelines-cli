use azure_devops_cli::client::models::*;

fn load_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{name}"))
        .unwrap_or_else(|e| panic!("Failed to load fixture {name}: {e}"))
}

#[test]
fn deserialize_definitions_fixture() {
    let json = load_fixture("definitions.json");
    let resp: ListResponse<PipelineDefinition> = serde_json::from_str(&json).unwrap();
    assert_eq!(resp.count, Some(2));
    assert_eq!(resp.value.len(), 2);
    assert_eq!(resp.value[0].name, "CI Pipeline");
    assert_eq!(resp.value[1].path, "\\Infra");
}

#[test]
fn deserialize_builds_fixture() {
    let json = load_fixture("builds.json");
    let resp: BuildListResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp.count, 2);
    assert_eq!(resp.value.len(), 2);

    let completed = &resp.value[0];
    assert_eq!(completed.id, 100);
    assert_eq!(completed.status, BuildStatus::Completed);
    assert_eq!(completed.result, Some(BuildResult::Succeeded));
    assert!(completed.finish_time.is_some());
    assert_eq!(completed.requestor(), "Jane Doe");
    assert_eq!(completed.short_branch(), "main");

    let in_progress = &resp.value[1];
    assert_eq!(in_progress.id, 101);
    assert_eq!(in_progress.status, BuildStatus::InProgress);
    assert!(in_progress.result.is_none());
    assert!(in_progress.finish_time.is_none());
}

#[test]
fn deserialize_timeline_fixture() {
    let json = load_fixture("timeline.json");
    let timeline: BuildTimeline = serde_json::from_str(&json).unwrap();
    assert!(!timeline.records.is_empty());

    // Check stage record
    let stage = timeline
        .records
        .iter()
        .find(|r| r.record_type == "Stage")
        .unwrap();
    assert_eq!(stage.name, "Build");
    assert!(stage.parent_id.is_none());

    // Check task with log
    let task = timeline
        .records
        .iter()
        .find(|r| r.record_type == "Task")
        .unwrap();
    assert!(task.log.is_some());
    assert!(task.parent_id.is_some());
}

#[test]
fn deserialize_approvals_fixture() {
    let json = load_fixture("approvals.json");
    let resp: ApprovalListResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp.count, 1);
    assert_eq!(resp.value[0].id, "approval-1");
    assert_eq!(resp.value[0].status, "pending");
    assert!(!resp.value[0].steps.is_empty());
}

#[test]
fn load_log_fixture() {
    let content = load_fixture("log.txt");
    let lines: Vec<&str> = content.lines().collect();
    assert!(lines.len() >= 3);
    assert!(lines[0].contains("Starting"));
}

#[test]
fn deserialize_pull_requests_fixture() {
    let json = load_fixture("pull_requests.json");
    let resp: PullRequestListResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp.count, Some(3));
    assert_eq!(resp.value.len(), 3);

    let active_pr = &resp.value[0];
    assert_eq!(active_pr.pull_request_id, 42);
    assert_eq!(active_pr.title, "Add feature X");
    assert!(active_pr.is_active());
    assert!(!active_pr.is_draft);
    assert_eq!(active_pr.repo_name(), "frontend");
    assert_eq!(active_pr.short_source_branch(), "feat/x");
    assert_eq!(active_pr.short_target_branch(), "main");
    assert_eq!(active_pr.reviewers.len(), 2);
    assert_eq!(active_pr.reviewers[0].vote, 10);
    assert!(active_pr.reviewers[0].is_required);
    assert_eq!(active_pr.labels.len(), 1);

    let draft_pr = &resp.value[1];
    assert_eq!(draft_pr.pull_request_id, 43);
    assert!(draft_pr.is_draft);
    assert_eq!(draft_pr.reviewers[0].vote, -5);

    let completed_pr = &resp.value[2];
    assert_eq!(completed_pr.pull_request_id, 44);
    assert!(!completed_pr.is_active());
}

#[test]
fn deserialize_pull_request_threads_fixture() {
    let json = load_fixture("pull_request_threads.json");
    let resp: PullRequestThreadListResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp.count, Some(3));
    assert_eq!(resp.value.len(), 3);

    let active_thread = &resp.value[0];
    assert_eq!(active_thread.id, 1);
    assert!(active_thread.is_active());
    assert_eq!(active_thread.comments.len(), 2);

    let closed_thread = &resp.value[1];
    assert!(!closed_thread.is_active());

    let empty_thread = &resp.value[2];
    assert!(empty_thread.is_active());
    assert!(empty_thread.comments.is_empty());
}

#[test]
fn deserialize_connection_data_fixture() {
    let json = load_fixture("connection_data.json");
    let cd: ConnectionData = serde_json::from_str(&json).unwrap();
    assert_eq!(cd.user_id(), Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
    assert_eq!(
        cd.authenticated_user.unwrap().provider_display_name,
        Some("Alice Smith".to_string())
    );
}
