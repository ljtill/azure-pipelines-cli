//! Dependency-free scale regression tests for render and rebuild paths.

use std::collections::BTreeMap;
use std::path::PathBuf;

use azure_devops_cli::client::models::{
    BacklogLevelConfiguration, BuildResult, BuildStatus, BuildTimeline, LogReference, PullRequest,
    TaskState, TimelineRecord, WorkItem, WorkItemFields, WorkItemTypeReference,
};
use azure_devops_cli::components::boards::Boards;
use azure_devops_cli::components::dashboard::Dashboard;
use azure_devops_cli::components::log_viewer::{self, LogViewer, TimelineRow};
use azure_devops_cli::components::pipelines::{PipelineRow, Pipelines};
use azure_devops_cli::state::{
    App, DashboardPullRequestsState, DashboardWorkItemsState, PinnedWorkItemsState, View,
};
use azure_devops_cli::test_helpers::{
    make_build, make_config, make_definition, make_pull_request, make_timeline_record,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

// --- Helpers ---

fn buffer_to_string(buffer: &Buffer) -> String {
    let mut output = String::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        output.push_str(line.trim_end_matches(' '));
        output.push('\n');
    }
    output
}

fn test_app() -> App {
    let config = make_config();
    App::new(
        &config.devops.connection.organization,
        &config.devops.connection.project,
        &config,
        PathBuf::from("test-config.toml"),
    )
}

fn backlog(name: &str) -> BacklogLevelConfiguration {
    BacklogLevelConfiguration {
        id: format!("backlog-{name}"),
        name: name.to_string(),
        rank: 1,
        work_item_count_limit: None,
        work_item_types: vec![WorkItemTypeReference {
            name: name.to_string(),
            url: None,
        }],
        default_work_item_type: None,
        color: None,
        is_hidden: false,
        backlog_type: None,
    }
}

fn work_item(id: u32, parent_id: Option<u32>, title: &str, rank: u32) -> WorkItem {
    WorkItem {
        id,
        rev: Some(1),
        fields: WorkItemFields {
            title: title.to_string(),
            work_item_type: if parent_id.is_none() {
                "Epic".to_string()
            } else {
                "Feature".to_string()
            },
            state: Some("Active".to_string()),
            parent: parent_id,
            stack_rank: Some(f64::from(rank)),
            ..WorkItemFields::default()
        },
        relations: Vec::new(),
        url: None,
    }
}

fn board_work_items(root_count: u32, children_per_root: u32) -> Vec<WorkItem> {
    let item_count = root_count * (children_per_root + 1);
    let mut items = Vec::with_capacity(item_count as usize);

    for root in 0..root_count {
        let root_id = (root * 100) + 1;
        items.push(work_item(
            root_id,
            None,
            &format!("Scale root {root:04}"),
            root_id,
        ));

        for child in 0..children_per_root {
            let child_id = root_id + child + 1;
            items.push(work_item(
                child_id,
                Some(root_id),
                &format!("Scale child {root:04}-{child:02}"),
                child_id,
            ));
        }
    }

    items
}

fn pipeline_definitions(
    definition_count: u32,
) -> Vec<azure_devops_cli::client::models::PipelineDefinition> {
    (0..definition_count)
        .map(|index| {
            let team = index % 50;
            let service = (index / 50) % 10;
            make_definition(
                index + 1,
                &format!("Pipeline {index:04}"),
                &format!("\\Team{team:02}\\Service{service:02}"),
            )
        })
        .collect()
}

fn timeline_record(
    id: &str,
    record_type: &str,
    parent_id: Option<&str>,
    name: &str,
    order: i32,
    log_id: Option<u32>,
) -> TimelineRecord {
    let mut record = make_timeline_record(
        id,
        record_type,
        parent_id,
        name,
        order,
        Some(TaskState::Completed),
        Some(BuildResult::Succeeded),
    );
    record.log = log_id.map(|id| LogReference { id });
    record
}

fn nested_timeline(stages: u32, phases_per_stage: u32, tasks_per_phase: u32) -> BuildTimeline {
    let record_count = stages * (1 + (phases_per_stage * 2) + (phases_per_stage * tasks_per_phase));
    let mut records = Vec::with_capacity(record_count as usize);

    for stage in 0..stages {
        let stage_id = format!("stage-{stage:02}");
        records.push(timeline_record(
            &stage_id,
            "Stage",
            None,
            &format!("Stage {stage:02}"),
            stage as i32 + 1,
            None,
        ));

        for phase in 0..phases_per_stage {
            let phase_id = format!("phase-{stage:02}-{phase:02}");
            records.push(timeline_record(
                &phase_id,
                "Phase",
                Some(&stage_id),
                &format!("Phase {stage:02}-{phase:02}"),
                phase as i32 + 1,
                None,
            ));

            let nested_job_id = format!("agent-job-{stage:02}-{phase:02}");
            records.push(timeline_record(
                &nested_job_id,
                "Job",
                Some(&phase_id),
                &format!("Agent Job {stage:02}-{phase:02}"),
                1,
                None,
            ));

            for task in 0..tasks_per_phase {
                let task_id = format!("task-{stage:02}-{phase:02}-{task:02}");
                let log_id = (stage * phases_per_stage * tasks_per_phase)
                    + (phase * tasks_per_phase)
                    + task
                    + 1;
                records.push(timeline_record(
                    &task_id,
                    "Task",
                    Some(&nested_job_id),
                    &format!("Task {stage:02}-{phase:02}-{task:02}"),
                    task as i32 + 1,
                    Some(log_id),
                ));
            }
        }
    }

    BuildTimeline { records }
}

// --- Scale regressions ---

#[test]
fn log_render_large_buffer_uses_visible_window() {
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    let mut app = test_app();
    app.navigate_to_log_viewer(make_build(
        42,
        BuildStatus::Completed,
        Some(BuildResult::Succeeded),
    ));

    let line_count = 20_000;
    let log = (0..line_count)
        .map(|line| format!("render-scale-line-{line:05}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.log_viewer.set_log_content(&log);

    terminal
        .draw(|frame| {
            log_viewer::draw_log_viewer(frame, &mut app, frame.area());
        })
        .unwrap();

    let rendered = buffer_to_string(terminal.backend().buffer());
    assert!(rendered.contains("render-scale-line-19999"));
    assert!(!rendered.contains("render-scale-line-00000"));
    assert!(rendered.matches("render-scale-line-").count() <= 20);
}

#[test]
fn boards_rebuild_large_tree_keeps_collapsed_and_search_rows_bounded() {
    let root_count = 1_000;
    let children_per_root = 4;
    let mut boards = Boards::default();

    boards.set_data(
        "Project Team".to_string(),
        vec![backlog("Epics"), backlog("Features")],
        board_work_items(root_count, children_per_root),
        "",
    );

    assert_eq!(boards.items.len(), 5_000);
    assert_eq!(boards.rows.len(), root_count as usize);
    assert_eq!(boards.nav.len(), boards.rows.len());

    boards.rebuild("child 0999-03");

    let visible_ids = boards
        .rows
        .iter()
        .map(|row| row.work_item_id)
        .collect::<Vec<_>>();
    assert_eq!(visible_ids, vec![99_901, 99_905]);
    assert_eq!(boards.nav.len(), boards.rows.len());
}

#[test]
fn pipelines_rebuild_large_grouped_set_keeps_rows_deterministic() {
    let definitions = pipeline_definitions(5_000);
    let mut pipelines = Pipelines::default();

    pipelines.rebuild(&definitions, &BTreeMap::new(), &[], &[], &[], "");

    let expected_folder_headers = 50 + (50 * 10);
    assert_eq!(
        pipelines.rows.len(),
        definitions.len() + expected_folder_headers
    );
    assert_eq!(pipelines.nav.len(), pipelines.rows.len());

    pipelines.rebuild(
        &definitions,
        &BTreeMap::new(),
        &[],
        &[],
        &[],
        "Pipeline 4999",
    );

    assert_eq!(pipelines.rows.len(), 3);
    assert!(matches!(
        &pipelines.rows[0],
        PipelineRow::FolderHeader { label, depth: 0, .. } if label == "Team49"
    ));
    assert!(matches!(
        &pipelines.rows[1],
        PipelineRow::FolderHeader { label, depth: 1, .. } if label == "Service09"
    ));
    assert!(matches!(
        &pipelines.rows[2],
        PipelineRow::Pipeline { definition_id, depth: 2, .. } if *definition_id == 5_000
    ));
    assert_eq!(pipelines.nav.len(), pipelines.rows.len());
}

#[test]
fn dashboard_rebuild_large_inputs_keeps_capped_sections_deterministic() {
    let definitions = pipeline_definitions(2_000);
    let pinned_ids = (1..=2_000).collect::<Vec<_>>();
    let pull_requests: Vec<PullRequest> = (0..250)
        .map(|index| make_pull_request(index, &format!("Scale PR {index:03}"), "active", "repo"))
        .collect();
    let pinned_work_items = (0..75)
        .map(|index| {
            work_item(
                10_000 + index,
                None,
                &format!("Pinned item {index:03}"),
                index,
            )
        })
        .collect::<Vec<_>>();
    let active_work_items = (0..250)
        .map(|index| {
            work_item(
                20_000 + index,
                None,
                &format!("Active item {index:03}"),
                index,
            )
        })
        .collect::<Vec<_>>();
    let mut dashboard = Dashboard::default();

    dashboard.rebuild(
        &definitions,
        &BTreeMap::new(),
        &pinned_ids,
        &DashboardPullRequestsState::Ready(pull_requests),
        &DashboardWorkItemsState::Ready(active_work_items),
        &PinnedWorkItemsState::Ready(pinned_work_items),
    );

    assert_eq!(dashboard.section_ranges[0].len(), 2_000);
    assert_eq!(dashboard.section_ranges[1].len(), 75);
    assert_eq!(dashboard.section_ranges[2].len(), 10);
    assert_eq!(dashboard.section_ranges[3].len(), 10);
    assert_eq!(dashboard.nav.len(), dashboard.rows.len());
}

#[test]
fn timeline_flattening_large_nested_jobs_omits_intermediate_job_rows() {
    let stages = 30;
    let phases_per_stage = 6;
    let tasks_per_phase = 8;
    let timeline = nested_timeline(stages, phases_per_stage, tasks_per_phase);
    let mut viewer = LogViewer::new_for_build(
        make_build(99, BuildStatus::Completed, Some(BuildResult::Succeeded)),
        View::BuildHistory,
        1,
    );
    viewer.set_build_timeline(timeline);
    viewer.rebuild_timeline_rows();

    assert_eq!(viewer.timeline_rows().len(), stages as usize);

    let stage_ids = viewer.collapsed_stages.iter().cloned().collect::<Vec<_>>();
    for id in &stage_ids {
        viewer.expand_stage(id);
    }
    let job_ids = viewer.collapsed_jobs.iter().cloned().collect::<Vec<_>>();
    for id in &job_ids {
        viewer.expand_job(id);
    }
    viewer.rebuild_timeline_rows();

    let expected_rows = stages * (1 + (phases_per_stage * (1 + tasks_per_phase)));
    assert_eq!(viewer.timeline_rows().len(), expected_rows as usize);
    assert_eq!(viewer.nav().len(), viewer.timeline_rows().len());
    assert!(
        viewer
            .timeline_rows()
            .iter()
            .any(|row| matches!(row, TimelineRow::Task { name, .. } if name == "Task 29-05-07"))
    );
    assert!(
        !viewer.timeline_rows().iter().any(
            |row| matches!(row, TimelineRow::Job { name, .. } if name.starts_with("Agent Job"))
        )
    );
}
