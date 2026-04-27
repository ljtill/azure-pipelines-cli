//! Factory functions for building test fixtures used by unit and integration tests.

use std::path::PathBuf;

use crate::client::models::{
    Build, BuildDefinitionRef, BuildResult, BuildStatus, BuildTimeline, LogReference,
    PipelineDefinition, PullRequest, PullRequestThread, Reviewer, TaskState, TimelineRecord,
};
use crate::config::{
    Config, ConnectionConfig, ConnectionTimeoutConfig, DisplayConfig, FiltersConfig, LoggingConfig,
    NotificationsConfig, UpdateConfig,
};
use crate::shared::availability::Availability;
use crate::state::App;

/// Re-exports `AppMessage` for use from integration tests. The underlying
/// `state::messages` module is crate-private; this indirection keeps the
/// visibility change isolated to `test_helpers`.
#[doc(hidden)]
pub use crate::state::messages::{AppMessage, RefreshOutcome, RefreshSource};

/// Creates a [`Build`] with the given id, status, and optional result.
pub fn make_build(id: u32, status: BuildStatus, result: Option<BuildResult>) -> Build {
    Build {
        id,
        build_number: format!("{id}"),
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
        reason: None,
        trigger_info: None,
    }
}

/// Creates a [`PipelineDefinition`] with the given id, name, and folder path.
pub fn make_definition(id: u32, name: &str, path: &str) -> PipelineDefinition {
    PipelineDefinition {
        id,
        name: name.to_string(),
        path: path.to_string(),
        queue_status: None,
        latest_build: None,
    }
}

/// Creates a [`TimelineRecord`] with the given properties.
pub fn make_timeline_record(
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
        parent_id: parent_id.map(std::string::ToString::to_string),
        name: name.to_string(),
        identifier: None,
        record_type: record_type.to_string(),
        state,
        result,
        order: Some(order),
        log: None,
    }
}

/// Creates a two-stage [`BuildTimeline`] (Build → Deploy) with phases and tasks.
pub fn make_simple_timeline() -> BuildTimeline {
    let completed = Some(TaskState::Completed);
    let succeeded = Some(BuildResult::Succeeded);

    let mut records = Vec::new();

    // Stage: Build.
    records.push(make_timeline_record(
        "stage-build",
        "Stage",
        None,
        "Build",
        1,
        completed,
        succeeded,
    ));

    // Phase under Build.
    records.push(make_timeline_record(
        "phase-build",
        "Phase",
        Some("stage-build"),
        "Build Job",
        1,
        completed,
        succeeded,
    ));

    // Tasks under Build phase.
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

    // Stage: Deploy.
    records.push(make_timeline_record(
        "stage-deploy",
        "Stage",
        None,
        "Deploy",
        2,
        completed,
        succeeded,
    ));

    // Phase under Deploy.
    records.push(make_timeline_record(
        "phase-deploy",
        "Phase",
        Some("stage-deploy"),
        "Deploy Job",
        1,
        completed,
        succeeded,
    ));

    // Tasks under Deploy phase.
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

/// Creates a [`PullRequest`] with the given id, title, status, and repository name.
pub fn make_pull_request(id: u32, title: &str, status: &str, repo_name: &str) -> PullRequest {
    PullRequest {
        pull_request_id: id,
        title: title.to_string(),
        description: None,
        status: status.to_string(),
        created_by: Some(crate::client::models::IdentityRef {
            id: None,
            unique_name: None,
            descriptor: None,
            display_name: "Test User".to_string(),
        }),
        creation_date: None,
        source_ref_name: Some("refs/heads/feat/test".to_string()),
        target_ref_name: Some("refs/heads/main".to_string()),
        repository: Some(crate::client::models::GitRepositoryRef {
            id: format!("repo-{id}"),
            name: repo_name.to_string(),
        }),
        reviewers: vec![],
        merge_status: None,
        is_draft: false,
        url: None,
        labels: vec![],
    }
}

/// Creates a [`Reviewer`] with the given display name and vote.
pub fn make_reviewer(name: &str, vote: i32) -> Reviewer {
    Reviewer {
        id: None,
        display_name: name.to_string(),
        unique_name: None,
        vote,
        is_required: false,
        has_declined: false,
    }
}

/// Creates a [`PullRequestThread`] with the given id, status, and number of placeholder comments.
pub fn make_pr_thread(id: u32, status: &str, comment_count: usize) -> PullRequestThread {
    let comments = (0..comment_count)
        .map(|i| crate::client::models::PullRequestComment {
            id: i as u32 + 1,
            author: None,
            content: Some(format!("Comment {}", i + 1)),
            published_date: None,
            comment_type: Some("text".to_string()),
        })
        .collect();
    PullRequestThread {
        id,
        status: Some(status.to_string()),
        comments,
        published_date: None,
        last_updated_date: None,
    }
}

/// Creates a minimal [`Config`] with default test values.
pub fn make_config() -> Config {
    Config {
        schema_version: Some(crate::config::CURRENT_SCHEMA_VERSION),
        devops: crate::config::DevOpsConfig {
            connection: ConnectionConfig {
                organization: "testorg".to_string(),
                project: "testproj".to_string(),
                timeouts: ConnectionTimeoutConfig::default(),
            },
            filters: FiltersConfig::default(),
            update: UpdateConfig::default(),
            logging: LoggingConfig::default(),
            notifications: NotificationsConfig::default(),
            display: DisplayConfig::default(),
        },
    }
}

/// Creates a fully populated [`App`] with definitions, builds, and rebuilt views.
pub fn make_app() -> App {
    let config = make_config();
    let mut app = App::new(
        &config.devops.connection.organization,
        &config.devops.connection.project,
        &config,
        PathBuf::from("/tmp/test-config.toml"),
    );

    // Creates 3 definitions across two folders.
    let def1 = make_definition(1, "CI Pipeline", "\\");
    let def2 = make_definition(2, "Deploy Pipeline", "\\Infra");
    let def3 = make_definition(3, "Infra Lint", "\\Infra");

    // Creates 3 recent builds, one per definition.
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

    app.core
        .data
        .apply_refresh(vec![def1, def2, def3], vec![b1, b2, b3], Vec::new());
    app.core.availability.definitions = Availability::fresh(app.core.data.definitions.clone());
    app.core.availability.recent_builds = Availability::fresh(app.core.data.recent_builds.clone());
    app.core.availability.pending_approvals =
        Availability::fresh(app.core.data.pending_approvals.clone());
    app.core.availability.retention_leases =
        Availability::fresh(app.core.retention_leases.leases.clone());
    app.core.availability.refresh = Availability::fresh(crate::state::CoreDataSnapshot::from_data(
        &app.core.data,
        &app.core.retention_leases,
    ));

    app.rebuild_dashboard();
    app.rebuild_pipelines();

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

        assert_eq!(
            tl.records
                .iter()
                .filter(|r| r.record_type == "Stage")
                .count(),
            2
        );

        assert_eq!(
            tl.records
                .iter()
                .filter(|r| r.record_type == "Phase")
                .count(),
            2
        );

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
        assert_eq!(c.devops.connection.organization, "testorg");
        assert_eq!(c.devops.connection.project, "testproj");
    }

    #[test]
    fn make_app_populates_state() {
        let app = make_app();
        assert_eq!(app.core.data.definitions.len(), 3);
        assert_eq!(app.core.data.recent_builds.len(), 3);
        assert!(!app.dashboard.rows.is_empty());
        assert!(!app.pipelines.rows.is_empty());
    }

    #[test]
    fn make_pull_request_sets_fields() {
        let pr = make_pull_request(42, "Add feature X", "active", "my-repo");
        assert_eq!(pr.pull_request_id, 42);
        assert_eq!(pr.title, "Add feature X");
        assert_eq!(pr.status, "active");
        assert_eq!(pr.repo_name(), "my-repo");
        assert_eq!(pr.author(), "Test User");
        assert!(pr.is_active());
    }

    #[test]
    fn make_reviewer_sets_fields() {
        let r = make_reviewer("Alice", 10);
        assert_eq!(r.display_name, "Alice");
        assert_eq!(r.vote, 10);
    }

    #[test]
    fn make_pr_thread_sets_fields() {
        let t = make_pr_thread(1, "active", 3);
        assert_eq!(t.id, 1);
        assert_eq!(t.status.as_deref(), Some("active"));
        assert_eq!(t.comments.len(), 3);
        assert!(t.is_active());
    }
}
