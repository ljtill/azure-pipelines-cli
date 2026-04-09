use azure_pipelines_cli::api::models::*;

fn load_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{}", name))
        .unwrap_or_else(|e| panic!("Failed to load fixture {}: {}", name, e))
}

#[test]
fn deserialize_definitions_fixture() {
    let json = load_fixture("definitions.json");
    let resp: DefinitionListResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp.count, 2);
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
