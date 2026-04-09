use crate::api::models::*;
use crate::app::App;
use crate::config::{AzureDevOpsConfig, Config, DisplayConfig, FiltersConfig, UpdateConfig};

pub(crate) fn make_build(id: u32, status: BuildStatus, result: Option<BuildResult>) -> Build {
    Build {
        id,
        build_number: format!("{}", id),
        status,
        result,
        queue_time: None,
        start_time: None,
        finish_time: None,
        definition: BuildDefinitionRef {
            id: 1,
            name: "test-pipeline".to_string(),
        },
        source_branch: Some("refs/heads/main".to_string()),
        requested_for: None,
    }
}

pub(crate) fn make_definition(id: u32, name: &str, path: &str) -> PipelineDefinition {
    PipelineDefinition {
        id,
        name: name.to_string(),
        path: path.to_string(),
        queue_status: None,
    }
}

pub(crate) fn make_timeline_record(
    id: &str,
    record_type: &str,
    parent_id: Option<&str>,
    name: &str,
    order: i32,
    state: Option<TaskState>,
    result: Option<BuildResult>,
) -> TimelineRecord {
    TimelineRecord {
        id: id.to_string(),
        parent_id: parent_id.map(|s| s.to_string()),
        name: name.to_string(),
        identifier: None,
        record_type: record_type.to_string(),
        state,
        result,
        order: Some(order),
        log: None,
    }
}

pub(crate) fn make_simple_timeline() -> BuildTimeline {
    let completed = Some(TaskState::Completed);
    let succeeded = Some(BuildResult::Succeeded);

    let mut records = Vec::new();

    // Stage: Build
    records.push(make_timeline_record(
        "stage-build",
        "Stage",
        None,
        "Build",
        1,
        completed,
        succeeded,
    ));

    // Phase under Build
    records.push(make_timeline_record(
        "phase-build",
        "Phase",
        Some("stage-build"),
        "Build Job",
        1,
        completed,
        succeeded,
    ));

    // Tasks under Build phase
    let mut task1 = make_timeline_record(
        "task-build-1",
        "Task",
        Some("phase-build"),
        "Checkout",
        1,
        completed,
        succeeded,
    );
    task1.log = Some(LogReference { id: 10 });
    records.push(task1);

    let mut task2 = make_timeline_record(
        "task-build-2",
        "Task",
        Some("phase-build"),
        "Build Solution",
        2,
        completed,
        succeeded,
    );
    task2.log = Some(LogReference { id: 11 });
    records.push(task2);

    // Stage: Deploy
    records.push(make_timeline_record(
        "stage-deploy",
        "Stage",
        None,
        "Deploy",
        2,
        completed,
        succeeded,
    ));

    // Phase under Deploy
    records.push(make_timeline_record(
        "phase-deploy",
        "Phase",
        Some("stage-deploy"),
        "Deploy Job",
        1,
        completed,
        succeeded,
    ));

    // Tasks under Deploy phase
    let mut task3 = make_timeline_record(
        "task-deploy-1",
        "Task",
        Some("phase-deploy"),
        "Download Artifacts",
        1,
        completed,
        succeeded,
    );
    task3.log = Some(LogReference { id: 20 });
    records.push(task3);

    let mut task4 = make_timeline_record(
        "task-deploy-2",
        "Task",
        Some("phase-deploy"),
        "Deploy to Staging",
        2,
        completed,
        succeeded,
    );
    task4.log = Some(LogReference { id: 21 });
    records.push(task4);

    BuildTimeline { records }
}

pub(crate) fn make_config() -> Config {
    Config {
        azure_devops: AzureDevOpsConfig {
            organization: "testorg".to_string(),
            project: "testproj".to_string(),
        },
        display: DisplayConfig::default(),
        filters: FiltersConfig::default(),
        update: UpdateConfig::default(),
    }
}

pub(crate) fn make_app() -> App {
    let config = make_config();
    let mut app = App::new(
        &config.azure_devops.organization,
        &config.azure_devops.project,
        &config,
    );

    // 3 definitions across two folders
    let def1 = make_definition(1, "CI Pipeline", "\\");
    let def2 = make_definition(2, "Deploy Pipeline", "\\Infra");
    let def3 = make_definition(3, "Infra Lint", "\\Infra");
    app.definitions = vec![def1.clone(), def2.clone(), def3.clone()];

    // 3 recent builds, one per definition
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

    app.recent_builds = vec![b1.clone(), b2.clone(), b3.clone()];
    app.latest_builds_by_def.insert(1, b1);
    app.latest_builds_by_def.insert(2, b2);
    app.latest_builds_by_def.insert(3, b3);

    app.rebuild_dashboard_rows();
    app.rebuild_filtered_pipelines();

    app
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_build_sets_fields() {
        let b = make_build(42, BuildStatus::InProgress, None);
        assert_eq!(b.id, 42);
        assert_eq!(b.build_number, "42");
        assert_eq!(b.status, BuildStatus::InProgress);
        assert!(b.result.is_none());
    }

    #[test]
    fn make_definition_sets_fields() {
        let d = make_definition(7, "My Pipeline", "\\Ops");
        assert_eq!(d.id, 7);
        assert_eq!(d.name, "My Pipeline");
        assert_eq!(d.path, "\\Ops");
    }

    #[test]
    fn make_simple_timeline_structure() {
        let tl = make_simple_timeline();
        assert_eq!(tl.records.len(), 8);

        let stages: Vec<_> = tl
            .records
            .iter()
            .filter(|r| r.record_type == "Stage")
            .collect();
        assert_eq!(stages.len(), 2);

        let phases: Vec<_> = tl
            .records
            .iter()
            .filter(|r| r.record_type == "Phase")
            .collect();
        assert_eq!(phases.len(), 2);

        let tasks: Vec<_> = tl
            .records
            .iter()
            .filter(|r| r.record_type == "Task")
            .collect();
        assert_eq!(tasks.len(), 4);
        assert!(tasks.iter().all(|t| t.log.is_some()));
    }

    #[test]
    fn make_config_values() {
        let c = make_config();
        assert_eq!(c.azure_devops.organization, "testorg");
        assert_eq!(c.azure_devops.project, "testproj");
    }

    #[test]
    fn make_app_populates_state() {
        let app = make_app();
        assert_eq!(app.definitions.len(), 3);
        assert_eq!(app.recent_builds.len(), 3);
        assert!(!app.dashboard_rows.is_empty());
        assert!(!app.filtered_pipelines.is_empty());
    }
}
